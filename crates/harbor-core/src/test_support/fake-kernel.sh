#!/bin/sh
# No listeners or TUN. Model only the activation acknowledgement protocol.
if [ "$1" = check ]; then exit 0; fi
harbor_ack() {
    { printf '{"pid":%s,' "$$"; tail -c +2 activation-request.json; } > activation-ack.json.next
    mv activation-ack.json.next activation-ack.json
}
trap 'harbor_ack' HUP
harbor_ack
while :; do sleep 0.02; done
