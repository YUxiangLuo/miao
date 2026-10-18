#!/bin/sh
# No listeners or TUN. Model only the activation acknowledgement protocol.
if [ "$1" = check ]; then exit 0; fi
miao_ack() {
    { printf '{"pid":%s,' "$$"; tail -c +2 activation-request.json; } > activation-ack.json.next
    mv activation-ack.json.next activation-ack.json
}
trap 'miao_ack' HUP
miao_ack
while :; do sleep 0.02; done
