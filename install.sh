#!/bin/bash
# Sideral · macOS installer. Jardim Sonoro, 2026.
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AU="$HOME/Library/Audio/Plug-Ins/Components"
VST3="$HOME/Library/Audio/Plug-Ins/VST3"
CLAP="$HOME/Library/Audio/Plug-Ins/CLAP"
PRESETS="$HOME/Library/Audio/Presets/Jardim Sonoro/Sideral"
mkdir -p "$AU" "$VST3" "$CLAP" "$PRESETS"
echo "Sideral: aligning the instruments…"
for b in Sideral.component:"$AU" Sideral.vst3:"$VST3" Sideral.clap:"$CLAP"; do
  name="${b%%:*}"; dest="${b#*:}"
  if [ -d "$DIR/$name" ]; then
    rm -rf "$dest/$name"
    cp -R "$DIR/$name" "$dest/"
    xattr -cr "$dest/$name" 2>/dev/null || true
    echo "  -> $dest/$name"
  fi
done
if [ -d "$DIR/Presets" ]; then cp "$DIR"/Presets/*.preset "$PRESETS/" 2>/dev/null || true; fi
killall -9 AudioComponentRegistrar 2>/dev/null || true
echo "Done. Restart your DAW and look up."
