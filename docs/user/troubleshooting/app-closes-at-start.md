---
title: DesignCraft closes right after it starts
description: On Windows, DesignCraft shows a brief spinner and then closes with no message. Update, or force the DirectX 12 graphics backend.
order: 10
verified: 0.7.1
---

If DesignCraft flashes a spinner and closes, update to the [latest release](https://github.com/storytold/designcraft/releases/latest). If it still closes, set the environment variable `WGPU_BACKEND` to `dx12` and start it again.

## Symptoms

- On Windows, DesignCraft opens briefly with a spinner or a quick blue circle, then closes. No error message appears.
- Windows may record a crash that names a graphics driver file such as `atio6axx.dll` (AMD) or `igvk64.dll` (Intel).
- It happens with the installer and the portable version.

## Fix

1. Install the [latest release](https://github.com/storytold/designcraft/releases/latest). DesignCraft remembers a start that never showed a window and tries the next graphics backend on the following start. On Windows the order is DirectX 12, Vulkan, then OpenGL, and the status bar says when it switches. A reporter who could not start 0.5.0 on Windows 11 Enterprise wrote that 0.6.0 launched without trouble ([issue #273](https://github.com/storytold/designcraft/issues/273)).
2. If it still closes, set an environment variable named `WGPU_BACKEND` with the value `dx12`. Start DesignCraft from a shell where it is set, or add it in Windows settings for your account. People who hit this crash with an AMD graphics driver reported that DesignCraft starts with `WGPU_BACKEND=dx12`. `dx12` also avoids Intel's Vulkan driver (`igvk64.dll`), which crashed in [issue #192](https://github.com/storytold/designcraft/issues/192).
3. Start DesignCraft again.

Setting `WGPU_BACKEND` also turns off the automatic fallback. Other accepted values are `vulkan` and `gl`, and a comma-separated list lets the graphics library choose among them. Do not pick `vulkan` on Intel graphics or `gl` on AMD graphics: those are the drivers in the crash reports.

## Why this happens

DesignCraft draws the canvas on the CPU and uses the graphics card only to show the interface. Some graphics drivers crash while the window is being set up, and a crash in a driver ends the program before DesignCraft can react. One AMD laptop's Vulkan driver crashed on the first frame, which is why DirectX 12 is the default on Windows.

## Start the backend search over

DesignCraft keeps its record of failed backends in a file called `gpu.json` in its settings folder, the one that holds `ui.json`, `prefs.json` and the `logs` folder (`%APPDATA%\DesignCraft\logs\` is the log folder on Windows). Delete the file to start the search again. The list also starts over after every backend has failed, and with a new DesignCraft version.

## Still stuck?

Send the log. See [Report a problem](get-help.md) for where it is. Or ask in [Discord](https://discord.gg/artcraft).
