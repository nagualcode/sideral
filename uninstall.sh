#!/bin/bash
# Sideral · macOS uninstaller. The memory, as ever, is not kept.
rm -rf "$HOME/Library/Audio/Plug-Ins/Components/Sideral.component" \
       "$HOME/Library/Audio/Plug-Ins/VST3/Sideral.vst3" \
       "$HOME/Library/Audio/Plug-Ins/CLAP/Sideral.clap" \
       "$HOME/Library/Audio/Presets/Jardim Sonoro/Sideral"
killall -9 AudioComponentRegistrar 2>/dev/null || true
echo "Sideral has set below the horizon."
