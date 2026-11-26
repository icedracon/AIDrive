# Renders docs/demo/demo.gif: vhs docs/demo/demo.tape (from the repository root).
# On Windows VHS runs WSL bash; the hidden prelude points at Linux builds.
Output docs/demo/demo.gif
Set Shell bash
Set FontSize 15
Set Width 1280
Set Height 760
Set TypingSpeed 35ms
Set Theme "Catppuccin Mocha"

Hide
Type 'export AIDRIVE_API_BIN=$PWD/target/wsl/debug/aidrive-api AIDRIVE_MCP=$PWD/target/wsl/debug/aidrive-mcp-server PS1="$ "; clear'
Enter
Show

Type "bash scripts/demo.sh"
Sleep 400ms
Enter
Sleep 32s
