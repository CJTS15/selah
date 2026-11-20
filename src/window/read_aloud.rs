//! Read aloud with the bundled espeak-ng: verse by verse, the verse being read
//! highlighted, the next one synthesised while the current one plays.

use selah_core::speech;

use super::*;

#[derive(Debug, Default)]
pub struct ReadAloud {
    active: bool,
    paused: bool,
    /// The verse being read, as this translation numbers it.
    verse: Option<u32>,
    media: Option<gtk::MediaFile>,
    /// The next verse's audio, prepared while the current one plays.
    prepared: Option<(u32, glib::Bytes)>,
    /// Bumped on every jump or stop, so late audio is dropped.
    generation: u64,
    /// Set while moving to the next chapter, so opening it keeps reading.
    continuing: bool,
}

/// Runs espeak-ng with the text on stdin (never as an argument) and returns
/// a WAV with correct sizes.
async fn synthesize(
    voice: &str,
    words_per_minute: u32,
    text: String,
) -> Result<glib::Bytes, String> {
    let speed = words_per_minute.to_string();
    let args: [&std::ffi::OsStr; 7] = [
        "espeak-ng".as_ref(),
        "-v".as_ref(),
        voice.as_ref(),
        "-s".as_ref(),
        speed.as_ref(),
        "--stdin".as_ref(),
        "--stdout".as_ref(),
    ];
    let process = gio::Subprocess::newv(
        &args,
        gio::SubprocessFlags::STDIN_PIPE
            | gio::SubprocessFlags::STDOUT_PIPE
            | gio::SubprocessFlags::STDERR_SILENCE,
    )
    .map_err(|err| err.to_string())?;
    let (stdout, _) = process
        .communicate_future(Some(&glib::Bytes::from_owned(text.into_bytes())))
        .await
        .map_err(|err| err.to_string())?;
    if !process.is_successful() {
        return Err("espeak-ng failed".into());
    }
    let mut wav = stdout.ok_or("espeak-ng gave no audio")?.to_vec();
    speech::fix_wav_sizes(&mut wav);
    Ok(glib::Bytes::from_owned(wav))
}

impl SelahWindow {
    pub(super) fn setup_read_aloud(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("read-aloud")
                .activate(|win: &Self, _, _| win.toggle_reading())
                .build(),
            gio::ActionEntry::builder("read-from-here")
                .activate(|win: &Self, _, _| {
                    let verse = win.active_verse();
                    win.close_verse_actions();
                    win.start_reading(verse);
                })
                .build(),
            gio::ActionEntry::builder("read-previous")
                .activate(|win: &Self, _, _| win.skip_verse(false))
                .build(),
            gio::ActionEntry::builder("read-next")
                .activate(|win: &Self, _, _| win.skip_verse(true))
                .build(),
            gio::ActionEntry::builder("read-stop")
                .activate(|win: &Self, _, _| win.stop_reading())
                .build(),
        ]);
        // Created last, so the reading tint is drawn over highlights.
        self.imp()
            .text_view
            .buffer()
            .create_tag(Some("reading"), &[]);
        self.apply_reading_colour();
        adw::StyleManager::default().connect_accent_color_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.apply_reading_colour()
        ));
    }

    fn apply_reading_colour(&self) {
        if let Some(tag) = self.imp().text_view.buffer().tag_table().lookup("reading") {
            let mut colour = adw::StyleManager::default().accent_color().to_rgba();
            colour.set_alpha(0.3);
            tag.set_background_rgba(Some(&colour));
        }
    }

    /// The espeak-ng voice for the open translation, or why there is none.
    fn reading_voice(&self) -> Result<&'static str, String> {
        let imp = self.imp();
        let id = imp.translation_id.borrow();
        let translations = imp.translations.borrow();
        let info = translations.iter().find(|t| t.id == *id);
        let code = info.map_or("", |t| t.language_code.as_str());
        speech::voice_for_language(code).ok_or_else(|| {
            let language = info.map_or(String::new(), |t| t.language.clone());
            // Translators: {language} is a language name such as "Tagalog".
            gettext("Read aloud is not available in {language}").replace("{language}", &language)
        })
    }

    /// Ctrl+P and the menu: start, pause or resume.
    fn toggle_reading(&self) {
        let (active, paused) = {
            let state = self.imp().read_aloud.borrow();
            (state.active, state.paused)
        };
        if !active {
            self.start_reading(None);
            return;
        }
        let state = self.imp().read_aloud.borrow_mut();
        let paused = !paused;
        if let Some(media) = &state.media {
            media.set_playing(!paused);
        }
        drop(state);
        self.imp().read_aloud.borrow_mut().paused = paused;
        self.update_read_bar();
    }

    /// Reads the open chapter from `verse`, or from its first verse.
    fn start_reading(&self, verse: Option<u32>) {
        if let Err(reason) = self.reading_voice() {
            self.toast(&reason);
            return;
        }
        let first = verse.or_else(|| {
            self.imp()
                .chapter_content
                .borrow()
                .as_ref()?
                .verses
                .first()
                .map(|v| v.verse)
        });
        let Some(first) = first else {
            return;
        };
        {
            let mut state = self.imp().read_aloud.borrow_mut();
            state.active = true;
            state.paused = false;
        }
        self.imp().read_bar.set_revealed(true);
        self.read_verse(first);
    }

    fn stop_reading(&self) {
        let imp = self.imp();
        {
            let mut state = imp.read_aloud.borrow_mut();
            if !state.active {
                return;
            }
            state.generation += 1;
            if let Some(media) = state.media.take() {
                media.set_playing(false);
            }
            *state = ReadAloud {
                generation: state.generation,
                ..ReadAloud::default()
            };
        }
        let buffer = imp.text_view.buffer();
        let (start, end) = buffer.bounds();
        buffer.remove_tag_by_name("reading", &start, &end);
        imp.read_bar.set_revealed(false);
    }

    /// Verses of the open chapter in order, as (verse, text).
    fn chapter_verses(&self) -> Vec<(u32, String)> {
        self.imp()
            .chapter_content
            .borrow()
            .as_ref()
            .map(|c| c.verses.iter().map(|v| (v.verse, v.text.clone())).collect())
            .unwrap_or_default()
    }

    fn skip_verse(&self, forward: bool) {
        let Some(current) = self.imp().read_aloud.borrow().verse else {
            return;
        };
        let verses = self.chapter_verses();
        let Some(index) = verses.iter().position(|(v, _)| *v == current) else {
            return;
        };
        let target = if forward {
            verses.get(index + 1)
        } else {
            index.checked_sub(1).and_then(|i| verses.get(i))
        };
        match target {
            Some((verse, _)) => self.read_verse(*verse),
            None if forward => self.next_chapter_or_stop(),
            None => self.read_verse(current),
        }
    }

    /// Highlights `verse`, plays its audio, and prepares the next verse.
    fn read_verse(&self, verse: u32) {
        let imp = self.imp();
        let Ok(voice) = self.reading_voice() else {
            self.stop_reading();
            return;
        };
        let verses = self.chapter_verses();
        let Some(index) = verses.iter().position(|(v, _)| *v == verse) else {
            self.stop_reading();
            return;
        };
        let generation = {
            let mut state = imp.read_aloud.borrow_mut();
            state.generation += 1;
            state.verse = Some(verse);
            state.paused = false;
            if let Some(media) = state.media.take() {
                media.set_playing(false);
            }
            state.generation
        };
        self.mark_reading_verse();
        self.update_read_bar();

        let prepared = {
            let mut state = imp.read_aloud.borrow_mut();
            match state.prepared.take() {
                Some((v, audio)) if v == verse => Some(audio),
                _ => None,
            }
        };
        let speed = speech::words_per_minute(&self.settings().string("read-speed"));
        let text = verses[index].1.clone();
        let next = verses.get(index + 1).cloned();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let audio = match prepared {
                    Some(audio) => Ok(audio),
                    None => synthesize(voice, speed, text).await,
                };
                if win.imp().read_aloud.borrow().generation != generation {
                    return;
                }
                let audio = match audio {
                    Ok(audio) => audio,
                    Err(err) => {
                        glib::g_warning!("selah", "Read aloud: {err}");
                        win.toast(&gettext("Could not read this verse aloud"));
                        win.stop_reading();
                        return;
                    }
                };
                win.play(&audio, generation);
                // Prepare the next verse while this one plays.
                if let Some((next_verse, next_text)) = next
                    && let Ok(next_audio) = synthesize(voice, speed, next_text).await
                {
                    let mut state = win.imp().read_aloud.borrow_mut();
                    if state.active {
                        state.prepared = Some((next_verse, next_audio));
                    }
                }
            }
        ));
    }

    fn play(&self, audio: &glib::Bytes, generation: u64) {
        let media = gtk::MediaFile::for_input_stream(&gio::MemoryInputStream::from_bytes(audio));
        media.connect_ended_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |media| {
                if media.is_ended() && win.imp().read_aloud.borrow().generation == generation {
                    win.skip_verse(true);
                }
            }
        ));
        media.connect_error_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |media| {
                if let Some(err) = media.error() {
                    glib::g_warning!("selah", "Read aloud playback: {err}");
                    win.toast(&gettext("Could not play the audio"));
                    win.stop_reading();
                }
            }
        ));
        media.play();
        self.imp().read_aloud.borrow_mut().media = Some(media);
    }

    /// At the end of a chapter, open the next one and keep reading.
    fn next_chapter_or_stop(&self) {
        let imp = self.imp();
        let next = imp.current.get().and_then(|(book, chapter)| {
            store::step_chapter(&imp.books.borrow(), book, chapter, true)
        });
        match next {
            Some((book, chapter)) => {
                imp.read_aloud.borrow_mut().continuing = true;
                self.open_chapter(book, chapter, None);
            }
            None => self.stop_reading(),
        }
    }

    /// Opening a chapter stops reading, unless reading moved there itself.
    pub(super) fn reading_chapter_opened(&self) {
        if !self.imp().read_aloud.borrow().continuing {
            self.stop_reading();
        }
    }

    /// After a chapter is drawn: keep reading into it, or mark the verse again.
    pub(super) fn reading_chapter_shown(&self) {
        let (continuing, active) = {
            let mut state = self.imp().read_aloud.borrow_mut();
            (std::mem::take(&mut state.continuing), state.active)
        };
        if continuing {
            let first = self.chapter_verses().first().map(|(v, _)| *v);
            match first {
                Some(verse) => self.read_verse(verse),
                None => self.stop_reading(),
            }
        } else if active {
            // A redraw (preferences, notes) cleared the tag.
            self.mark_reading_verse();
        }
    }

    /// Tints the verse being read and keeps it in view.
    fn mark_reading_verse(&self) {
        let imp = self.imp();
        let buffer = imp.text_view.buffer();
        let (start, end) = buffer.bounds();
        buffer.remove_tag_by_name("reading", &start, &end);
        let Some(verse) = imp.read_aloud.borrow().verse else {
            return;
        };
        let Some((from, to)) = self.verse_range(verse) else {
            return;
        };
        let from_iter = buffer.iter_at_offset(from);
        buffer.apply_tag_by_name("reading", &from_iter, &buffer.iter_at_offset(to));
        let mark = match buffer.mark("reading") {
            Some(mark) => {
                buffer.move_mark(&mark, &from_iter);
                mark
            }
            None => buffer.create_mark(Some("reading"), &from_iter, true),
        };
        imp.text_view.scroll_to_mark(&mark, 0.1, false, 0.0, 0.0);
    }

    fn update_read_bar(&self) {
        let imp = self.imp();
        let (verse, paused) = {
            let state = imp.read_aloud.borrow();
            (state.verse, state.paused)
        };
        if let Some(title) = verse.and_then(|v| self.verse_title(v)) {
            imp.read_label.set_label(&title);
        }
        let (icon, label) = if paused {
            ("media-playback-start-symbolic", gettext("Resume"))
        } else {
            ("media-playback-pause-symbolic", gettext("Pause"))
        };
        imp.read_pause.set_icon_name(icon);
        imp.read_pause.set_tooltip_text(Some(&label));
        imp.read_pause
            .update_property(&[gtk::accessible::Property::Label(&label)]);
    }
}
