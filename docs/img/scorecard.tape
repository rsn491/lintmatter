Output "docs/img/scorecard.gif"
Set Shell "zsh"
Set FontFamily "Menlo"
Set FontSize 23
Set Width 1600
Set Height 760
Set Padding 30
Set Theme "Catppuccin Macchiato"
Set WindowBar "Colorful"
Set WindowBarSize 45
Set TypingSpeed 45ms

# Prepare the development binary and color environment without recording setup.
Hide
Type "unset NO_COLOR"
Enter
Type "lintmatter() { ./target/debug/lintmatter testdata/broken; tput civis; sleep 10; }"
Enter
Type "clear"
Enter
Show

Type "lintmatter testdata/broken"
Enter
Sleep 5s
