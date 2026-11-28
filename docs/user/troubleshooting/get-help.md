---
title: Report a problem
description: What to include and where to post when DesignCraft misbehaves or crashes.
order: 20
verified: 0.7.1
---

Post in the [issue tracker](https://github.com/storytold/designcraft/issues) or in [Discord](https://discord.gg/artcraft), and attach the log file. The log is the most useful thing you can send after a crash.

## What to include

- Your DesignCraft version and how you installed it (installer, portable, AppImage and so on).
- Your operating system and graphics card.
- What you did, what you expected and what happened.
- The log file `designcraft.log`.

## Where the log is

DesignCraft writes `logs/designcraft.log` in its settings folder, beside `ui.json` and `prefs.json`.

| System  | Log folder                                                              |
|---------|-------------------------------------------------------------------------|
| Windows | `%APPDATA%\DesignCraft\logs\`                                           |
| macOS   | `~/Library/Application Support/DesignCraft/logs/`                       |
| Linux   | `~/.config/designcraft/logs/` (or `$XDG_CONFIG_HOME/designcraft/logs/`) |

Each start moves the previous log to `designcraft.1.log`, and that one to `designcraft.2.log`. The log from a run that crashed survives the next start, so attach `designcraft.1.log` if you have restarted since. The file stops growing at 16 MiB.

## More detail in the log

Set the environment variable `RUST_LOG` to `debug` before starting DesignCraft to log more. The default is `info` for DesignCraft's own parts and `warn` for everything else.
