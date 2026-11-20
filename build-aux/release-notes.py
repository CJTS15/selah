"""Prints the metainfo release notes of one version as Markdown, for the
GitHub release. Fails if that version has no notes, so a release cannot go
out without them.

Usage: release-notes.py <version> <metainfo file>
"""

import sys
import xml.etree.ElementTree as ET


def main():
    version, path = sys.argv[1], sys.argv[2]
    root = ET.parse(path).getroot()
    release = root.find(f"releases/release[@version='{version}']")
    if release is None or release.find("description") is None:
        sys.exit(f"no release notes for {version} in {path}")
    lines = []
    for element in release.find("description"):
        text = " ".join("".join(element.itertext()).split())
        if element.tag == "p":
            lines += [text, ""]
        elif element.tag in ("ul", "ol"):
            lines += [f"- {' '.join(''.join(li.itertext()).split())}" for li in element]
            lines.append("")
    lines += [
        "## Install",
        "",
        f"Download `selah-{version}-x86_64.flatpak` below, then run:",
        "",
        "```sh",
        "flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo",
        f"flatpak install --user selah-{version}-x86_64.flatpak",
        "```",
        "",
        "Updates are not automatic yet: install the next release the same way.",
    ]
    print("\n".join(lines))


if __name__ == "__main__":
    main()
