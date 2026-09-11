#!/usr/bin/env bash
# 假 ssh：打印两行可控输出后，按第一个参数决定退出或挂起。
echo "FAKE-SSH-BANNER"
echo "line-two"
if [ "${1:-exit0}" = "hang" ]; then
  sleep 3600
fi
exit "${FAKE_SSH_EXIT:-0}"
