Output "docs/img/scorecard.gif"
Set Shell "zsh"
# Menlo on macOS; JetBrains Mono where Menlo isn't installed.
Set FontFamily "Menlo, JetBrains Mono"
# The README scales the GIF to the page width, so on a phone the text is only
# as legible as the terminal is narrow. Width fits the widest line of the
# report (128 columns, frontmatter.missing) at this FontSize and no more, and
# Height fits the report, so no frame space is spent on empty terminal.
# Change Width with FontSize, and grow both if the fixtures' output grows.
Set FontSize 16
Set Width 1350
Set Height 540
Set Padding 20
Set Theme "Catppuccin Macchiato"
Set WindowBar "Colorful"
Set WindowBarSize 40
Set TypingSpeed 70ms

# Prepare the development binary and color environment without recording setup.
Hide
Type "unset NO_COLOR"
Enter
Type "lintmatter() { sleep 0.9; ./target/debug/lintmatter testdata/broken; tput civis; sleep 10; }"
Enter
Type "clear"
Enter
Show

Type "lintmatter testdata/broken"
Enter
# The 0.9s pause inside the function above, then the report holds on screen.
Sleep 6s
