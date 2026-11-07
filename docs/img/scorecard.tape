Output "docs/img/scorecard.gif"
Set Shell "zsh"
# Menlo on macOS; JetBrains Mono where Menlo isn't installed.
Set FontFamily "Menlo, JetBrains Mono"
# The README and the homepage scale the GIF to the column width, so on a phone
# the text is only as legible as the terminal is narrow. This records a ~60
# column terminal (Width / glyph width at this FontSize): lintmatter wraps its
# findings to the terminal width, so nothing is lost, and the type stays near
# 10px on a 360px screen. Height fits the report so no frame space is spent on
# empty terminal. Change Width with FontSize, and grow Height if the fixtures'
# output grows.
Set FontSize 22
Set Width 840
Set Height 960
Set Padding 20
Set Theme "Catppuccin Macchiato"
Set WindowBar "Colorful"
Set WindowBarSize 40
Set TypingSpeed 90ms

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
