# JetBrains Mono

The font of the session terminals: the terminals crate paints the PCs' screens with it (spec
"The PC screen"), and the seated view's overlay uses the same file (spec "Assets").

- **Font:** JetBrains Mono by JetBrains, Philipp Nurullin and Konstantin Bulenkov, "Copyright 2020
  The JetBrains Mono Project Authors (https://github.com/JetBrains/JetBrainsMono)". Latin, Latin
  Extended, Greek, Cyrillic and Vietnamese, with box drawing and block elements.
- **Licence:** SIL Open Font License 1.1, in `OFL.txt` beside this note. Committed unmodified.
- **Source:** the Google Fonts repository, `ofl/jetbrainsmono/JetBrainsMono[wght].ttf` and
  `ofl/jetbrainsmono/OFL.txt` at commit `6e4b84c976cadb3c49a40fd9a1c203e4f7fcf2da`
  (https://github.com/google/fonts/tree/6e4b84c976cadb3c49a40fd9a1c203e4f7fcf2da/ofl/jetbrainsmono),
  version 2.211, built from upstream commit `19371302b95d218af43299bce79ddbddd0bc364d`. The font's sha256 is
  `48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda`.
- **Weights:** Google Fonts ships it only as a variable font (weight axis 100 to 800). The PC
  image draws weight 400, and 700 for bold cells.

The terminals crate embeds the file in the app binary (`include_bytes!`); nothing here copies it.
