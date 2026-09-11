Output "/Users/ricardo/code_repos/lintmatter/docs/img/scorecard.gif"
Set Shell "zsh"
Set FontFamily "Menlo"
Set FontSize 18
Set Width 960
Set Height 700
Set Padding 20
Set WindowBar "Colorful"
Set TypingSpeed 45ms

# Prepare the development binary and color environment without recording setup.
Hide
Type "unset NO_COLOR"
Enter
Type "lintmatter() { /Users/ricardo/code_repos/lintmatter/target/debug/lintmatter testdata/broken; tput civis; sleep 10; }"
Enter
Type "clear"
Enter
Show

Type "lintmatter testdata/broken"
Enter
Sleep 5s
