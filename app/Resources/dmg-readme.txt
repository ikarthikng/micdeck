MicDeck: read me first
======================

INSTALL
Drag MicDeck onto Applications, then open it from Applications. A mic icon appears in the menu bar (top right of the screen).

IF macOS SAYS IT CANNOT CHECK THE APP
MicDeck is not signed with a paid Apple developer account yet, so macOS asks you to confirm once:
  1. Open MicDeck. When macOS blocks it, click Done.
  2. Open System Settings > Privacy & Security, scroll down, and click "Open Anyway" next to MicDeck.
  3. Open MicDeck again and confirm.
(Alternative: run  xattr -dr com.apple.quarantine /Applications/MicDeck.app  in Terminal.)

LIGHTS AND MUTE INDICATOR
These work right away with a HyperX DuoCast. Nothing else to install.

MEETING MODE (OPTIONAL) NEEDS BLACKHOLE 2CH
Meeting mode cleans your voice and sends it to a virtual microphone called BlackHole 2ch. BlackHole is a free audio driver made by another developer (Existential Audio). MicDeck does not include it and does not install it. Read about it first and install it yourself only if you want Meeting mode:
  Website:     https://existential.audio/blackhole/
  Source code: https://github.com/ExistentialAudio/BlackHole
Without BlackHole everything else in MicDeck still works.

MORE
https://github.com/ikarthikng/micdeck
MicDeck is free software (GPL-3.0). Not affiliated with HyperX or HP.
