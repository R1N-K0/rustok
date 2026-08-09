# VHS tape for the README demo.
#
#   cargo build --release
#   vhs assets/demo.tape          # writes assets/demo.gif
#
# Install VHS from https://github.com/charmbracelet/vhs
# The tape assumes `tok` is on PATH; PATH is prepended below so a local
# release build is picked up without installing anything.

Output assets/demo.gif

Set Shell bash
Set FontSize 20
Set Width 1100
Set Height 720
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms
Set PlaybackSpeed 1.0

# Use the freshly built binary and keep colour on inside the recording.
Hide
Type "export PATH=$PWD/target/release:$PATH CLICOLOR_FORCE=1" Enter
Type "clear" Enter
Show

# 1. The pitch: how much of this repo fits in a context window?
Type "tok src/" Sleep 400ms Enter
Sleep 3.5s

Type "clear" Enter
Sleep 300ms

# 2. Where the tokens actually go, across a whole tree.
Type "tok . --by-ext" Sleep 400ms Enter
Sleep 3.5s

Type "clear" Enter
Sleep 300ms

# 3. Token boundaries, including multi-byte text.
Type "tok -t 'こんにちは、世界！Hello 🎉' --show" Sleep 400ms Enter
Sleep 3.5s

Type "tok -t 'hello world' --ids" Sleep 300ms Enter
Sleep 1.5s

Type "tok decode 24912 2375" Sleep 300ms Enter
Sleep 2s

Type "clear" Enter
Sleep 300ms

# 4. Scripting: a bare number, and a budget that fails the build.
Type "tok src/ --quiet" Sleep 300ms Enter
Sleep 1.5s

Type "tok src/ --limit 8000; echo \"exit=$?\"" Sleep 400ms Enter
Sleep 3s
