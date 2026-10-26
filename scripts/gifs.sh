#!/usr/bin/env bash
# Демо-гифки для README: docs/gifs/{spider,game,whip}.gif.
# Кадры рендерит само приложение без окна (`--record`), ffmpeg кладёт их на фон и собирает GIF.
# Использование: scripts/gifs.sh [spider|game|whip ...]   (по умолчанию все три)
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release --quiet
BIN=target/release/desktop_spider
OUT=docs/gifs
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"
FONT=/System/Library/Fonts/Supplemental/Arial.ttf

# Фон «рабочего стола» — однотонный (градиент в GIF раздувает файл дизерингом).
# Для режима «Просто плеть» — ещё условное зависшее окно: прямоугольник APP_RECT из src/record.rs,
# запись печатает его в пикселях.
desk() { echo "color=c=0x181c28:s=$1x$2:d=$3:r=20"; }
window_layer() {
  # Размеры деталей заданы для окна шириной 300 и пересчитываются под фактическую ширину.
  local x=$1 y=$2 w=$3 h=$4
  k() { echo $(( $1 * w / 300 )); }
  local t="drawbox=x=$((x+$(k 6))):y=$((y+$(k 10))):w=$w:h=$h:color=black@0.35:t=fill"
  t="$t,drawbox=x=$x:y=$y:w=$w:h=$h:color=0xf4f4f8:t=fill"
  t="$t,drawbox=x=$x:y=$y:w=$w:h=$(k 32):color=0xe2e2ea:t=fill"
  local i=0
  for c in 0xff6159 0xffbd2e 0x28c941; do
    t="$t,drawbox=x=$((x+$(k 10)+i*$(k 15))):y=$((y+$(k 11))):w=$(k 10):h=$(k 10):color=$c:t=fill"
    i=$((i+1))
  done
  t="$t,drawtext=fontfile=$FONT:text='Мессенджер (не отвечает)':fontsize=$(k 13):fontcolor=0x44445a:x=$((x+$(k 60))):y=$((y+$(k 10)))"
  for b in "20 50 140 0xdcdce6" "90 92 190 0xcfe0ff" "20 134 170 0xdcdce6" "60 176 220 0xcfe0ff" "20 218 120 0xdcdce6"; do
    set -- $b
    t="$t,drawbox=x=$((x+$(k $1))):y=$((y+$(k $2))):w=$(k $3):h=$(k 30):color=$4:t=fill"
  done
  echo "$t"
}

for mode in "${@:-spider game whip}"; do
  for m in $mode; do
    info=$("$BIN" --record "$m" "$TMP/$m.rgba")
    read -r W H FPS FRAMES AX AY AW AH REST <<<"$info"
    echo "$m: ${W}x${H}, $FRAMES кадров @ $FPS fps, $REST"
    secs=$(echo "$FRAMES / $FPS + 1" | bc)
    bg="$(desk "$W" "$H" "$secs")"
    [ "$m" = whip ] && bg="$bg,$(window_layer "$AX" "$AY" "$AW" "$AH")"
    ffmpeg -v error -y \
      -f rawvideo -pix_fmt rgba -s "${W}x${H}" -r "$FPS" -i "$TMP/$m.rgba" \
      -f lavfi -i "$bg" \
      -filter_complex "[1][0]overlay=shortest=1,scale=1000:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
      -loop 0 "$OUT/$m.gif"
    ls -la "$OUT/$m.gif" | awk '{print "  →", $NF, int($5/1024) " КБ"}'
  done
done
