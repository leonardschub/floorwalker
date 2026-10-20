# Security

Floorwalker runs on your machine with your user's rights. It reads Claude Code's
files under `~/.claude`, starts `claude --bg` and `claude attach`, types into a
session while you sit in, and can end a session (KILL) or send it a prompt (WORK
HARDER). A bug in any of these is a security problem, and so is anything that lets a
web page, a file or another program make the app do one of them.

## Reporting a vulnerability

Please report it privately through GitHub: open the repository's **Security** tab and
choose **Report a vulnerability**. Don't open a public issue for it.

Say what you found, how to reproduce it, and which commit and Claude Code version you
ran. You'll get an answer as soon as possible. This is a one-person project, so
please allow a little time.

## Supported versions

Only the latest commit on `main` is supported. There are no releases yet.

## In scope

- Typing into, starting, ending or messaging a session the user did not choose.
- The page (webview) reaching the network, the file system or a command it is not
  granted (`desktop/src-tauri/capabilities/default.json`).
- Opening links other than `http` and `https`, or opening one without a Ctrl+click.
- Session content leaving the machine.

Bugs in Claude Code itself belong to Anthropic; see
[Anthropic's responsible disclosure policy](https://www.anthropic.com/responsible-disclosure-policy).
