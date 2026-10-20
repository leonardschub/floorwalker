# Oswald

The world's only third-party file: the font of the room signs and the robots' model names
(spec "Assets", "Committed third-party files").

- **Font:** Oswald by Vernon Adams, Kalapi Gajjar and Cyreal, "Copyright 2016 The Oswald Project
  Authors (https://github.com/googlefonts/OswaldFont)". Latin, Latin Extended, Cyrillic and
  Vietnamese.
- **Licence:** SIL Open Font License 1.1, in `OFL.txt` beside this note. Committed unmodified.
- **Source:** the Google Fonts repository, `ofl/oswald/Oswald[wght].ttf` and `ofl/oswald/OFL.txt` at
  commit `8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5` (https://github.com/google/fonts/tree/8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5/ofl/oswald).
  The font's sha256 is `5b38c246e255a12f5712d640d56bcced0472466fc68983d2d0410ec0457c2817`.
- **Bold:** Google Fonts ships Oswald only as a variable font (weight axis 200 to 700). The world
  draws it at weight 700, Oswald Bold, through Bevy's `FontWeight`.

`regenerate.py` copies the font into the output as `fonts/oswald.ttf`.
