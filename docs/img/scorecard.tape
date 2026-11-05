Output "docs/img/scorecard.gif"
Set Shell "zsh"
# Menlo on macOS; JetBrains Mono where Menlo isn't installed.
Set FontFamily "Menlo, JetBrains Mono"
# The README scales the GIF to the page width, so on a phone the text is only
# as legible as the terminal is narrow. docs/demo keeps every line of the
# report within 76 columns; Width fits that at this FontSize with no wrapping.
# Change the two together, and grow Height if the prompt scrolls out of view.
Set FontSize 20
Set Width 1080
Set Height 540
Set Padding 20
Set Theme "Catppuccin Macchiato"
Set WindowBar "Colorful"
Set WindowBarSize 40
Set TypingSpeed 45ms

# Step into the demo repository and wrap the dev binary (run `cargo build`
# first) without recording the setup.
# --no-config keeps this repository's own .lintmatter.yaml, which excludes the
# demo, out of the run.
Hide
Type "unset NO_COLOR"
Enter
Type "cd docs/demo"
Enter
Type "lintmatter() { ../../target/debug/lintmatter --no-config; tput civis; sleep 10; }"
Enter
Type "clear"
Enter
Show

Type "lintmatter"
Enter
Sleep 5s
