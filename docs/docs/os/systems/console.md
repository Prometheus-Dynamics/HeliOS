---
title: Console
---

Interactive shell access for diagnostics.

The Console tab provides a PTY-backed interactive shell session running on the device. It is intended for low-level debugging and should be treated like SSH access.

:::caution
Console access can change device state. Be careful with commands that modify networking, services, or storage.
:::

## Sessions

On devices that support session management, the UI can create and manage multiple console sessions:

- Create a new session (spawns a shell on the device).
- Switch between sessions.
- Close a session (terminates the shell).
- Pop out a session into a separate window.

If session management is not available on your device build, the UI falls back to a direct console mode.

## Shell Details

The shell executable is resolved on the device (commonly `$SHELL`, then `/bin/bash`, then `/bin/sh`) and is started as an interactive shell.

## API (For Power Users)

- Session management: `GET/POST /v1/console/sessions`, `DELETE /v1/console/sessions/{session_id}`
- Terminal stream: `WS /v1/ws/console` (supports attaching to an existing session via a `sessionId` query parameter)
