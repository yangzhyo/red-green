#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT
cc -std=c11 -Wno-deprecated-declarations -DCJSON_NESTING_LIMIT=16 -fsanitize=address,undefined \
  -I"$root/components/cjson" -c "$root/components/cjson/cJSON.c" -o "$build/cjson.o"
cc -std=c11 -Wall -Wextra -Werror -Wno-unused-parameter -DCJSON_NESTING_LIMIT=16 \
  -fsanitize=address,undefined -g -I"$root/main" -I"$root/components/cjson" \
  "$root/main/device.c" "$build/cjson.o" "$root/tests/device_test.c" -lm -o "$build/device-test"
"$build/device-test"
cc -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -g -I"$root/main" \
  "$root/main/schedule.c" "$root/tests/schedule_test.c" -o "$build/schedule-test"
"$build/schedule-test"
cc -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -g -I"$root/main" \
  "$root/main/idle_screen.c" "$root/tests/idle_screen_test.c" -o "$build/idle-screen-test"
"$build/idle-screen-test"
cc -std=c11 -Wall -Wextra -Werror -Wno-unused-parameter -fsanitize=address,undefined -g \
  -I"$root/main" -I"$root/components/cjson" "$root/main/memory_sample.c" \
  "$build/cjson.o" "$root/tests/memory_sample_test.c" -lm -o "$build/memory-sample-test"
"$build/memory-sample-test"
