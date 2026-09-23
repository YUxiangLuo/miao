#!/usr/bin/env bash
# Local-only GoTUN comparison. All network/process changes live in disposable
# namespaces; no host routes, firewall, production API or database are touched.
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [[ ${1:-} != --inside ]]; then
  for tool in ip iperf3 jq nsenter unshare sudo; do command -v "$tool" >/dev/null; done
  output=${1:-"$ROOT/.cache/tun-benchmark-$(date +%s)"}
  mkdir -p "$output"
  output=$(realpath "$output")
  [[ -z $(ls -A "$output") ]] || { echo 'Output directory must be empty' >&2; exit 1; }
  exec sudo env MIAO_BENCH_HOST_NETNS="$(readlink /proc/self/ns/net)" \
    unshare --net --pid --fork --mount --mount-proc \
    bash "$0" --inside "$output"
fi
[[ -n ${MIAO_BENCH_HOST_NETNS:-} && $(readlink /proc/self/ns/net) != "$MIAO_BENCH_HOST_NETNS" ]] || exit 1
output=$2
kernel="$ROOT/embedded/sing-box-amd64"
server="$ROOT/embedded/sing-box-host"
[[ $(uname -m) == x86_64 && -x $kernel && -x $server ]]
export GOMAXPROCS=4
ip link set lo up
unshare --net sleep infinity &
peer=$!
for ((i=0;i<100;i++)); do
  [[ $(readlink "/proc/$peer/ns/net") != "$(readlink /proc/self/ns/net)" ]] && break
  sleep 0.01
done
[[ $(readlink "/proc/$peer/ns/net") != "$(readlink /proc/self/ns/net)" ]]
ip link add bench0 type veth peer name bench1
ip link set bench1 netns "$peer"
ip addr add 10.222.0.1/30 dev bench0
ip link set bench0 mtu 9000 up
ip route add default via 10.222.0.2 dev bench0
nsenter -t "$peer" -n ip link set lo up
nsenter -t "$peer" -n ip addr add 10.222.0.2/30 dev bench1
nsenter -t "$peer" -n ip link set bench1 mtu 9000 up
nsenter -t "$peer" -n ip addr add 198.18.1.2/32 dev lo
cat > "$output/server.json" <<'JSON'
{"log":{"level":"error"},"inbounds":[{"type":"shadowsocks","listen":"10.222.0.2","listen_port":8388,"method":"aes-128-gcm","password":"disposable-benchmark-only"}],"outbounds":[{"type":"direct"}]}
JSON
nsenter -t "$peer" -n "$server" run -c "$output/server.json" > "$output/server.log" 2>&1 &
nsenter -t "$peer" -n iperf3 -s -B 198.18.1.2 > "$output/iperf-server.log" 2>&1 &
sleep 0.3
"$kernel" version > "$output/version.txt"
"$server" version > "$output/server-version.txt"
iperf3 --version > "$output/iperf-version.txt"
getconf CLK_TCK > "$output/clock-ticks.txt"
uname -a > "$output/system.txt"
for round in 1 2 3; do
  # Reverse each pair's ordering to reduce warmup/order bias.
  modes='false true'
  if (( round % 2 == 0 )); then modes='true false'; fi
  for redirect in true false; do
    for queues in $modes; do
      prefix="$output/r$round-redirect-$redirect-mq-$queues"
      jq -n --argjson redirect "$redirect" --argjson queues "$queues" '{
        log:{level:"trace",timestamp:true,disabled:false},
        inbounds:[{type:"tun",interface_name:"sing-tun",address:["172.18.0.1/30"],mtu:9000,auto_route:true,strict_route:true,auto_redirect:$redirect,multi_queue:$queues,route_address:["198.18.1.2/32"]}],
        outbounds:[{type:"shadowsocks",tag:"proxy",server:"10.222.0.2",server_port:8388,method:"aes-128-gcm",password:"disposable-benchmark-only",bind_interface:"bench0"}],
        route:{default_interface:"bench0",final:"proxy"}
      }' > "$prefix.config.json"
      "$kernel" --disable-color run -c "$prefix.config.json" > "$prefix.log" 2>&1 &
      pid=$!
      for ((i=0;i<100;i++)); do
        grep -q 'sing-box started' "$prefix.log" && break
        sleep 0.05
      done
      grep -q 'sing-box started' "$prefix.log" || { cat "$prefix.log"; exit 1; }
      # Count attached TUN fds, rather than assuming the requested mode worked.
      grep -l '^iff:[[:space:]]*sing-tun' /proc/"$pid"/fdinfo/* | wc -l > "$prefix.queues"
      expected=1; if [[ $queues == true ]]; then expected=$GOMAXPROCS; fi
      [[ $(cat "$prefix.queues") == "$expected" ]]
      for protocol in tcp udp udp-stress; do
        for flows in 1 8; do
          for direction in forward reverse; do
            args=()
            # iperf applies -b per stream: hold aggregate offered load constant.
            rate=200
            if [[ $protocol == udp-stress ]]; then rate=2000; fi
            if [[ $protocol != tcp ]]; then args=(-u -b "$((rate / flows))M" -l 1200); fi
            if [[ $direction == reverse ]]; then args+=(-R); fi
            sample="$prefix-$protocol-$flows-$direction"
            ip -j -s link show sing-tun > "$sample.link-before.json"
            cat /proc/"$pid"/stat > "$sample.stat-before"
            timeout 15s iperf3 -c 198.18.1.2 -t 2 -P "$flows" -J "${args[@]}" > "$sample.json"
            jq -e 'has("error") | not' "$sample.json" >/dev/null
            cat /proc/"$pid"/stat > "$sample.stat-after"
            cat /proc/"$pid"/status > "$sample.status"
            ip -j -s link show sing-tun > "$sample.link-after.json"
          done
        done
      done
      # Only the child created above, inside this disposable PID namespace.
      kill -INT "$pid"
      wait "$pid"
      if ip link show sing-tun >/dev/null 2>&1; then echo 'TUN cleanup failed' >&2; exit 1; fi
      echo "round=$round auto_redirect=$redirect multi_queue=$queues completed"
    done
  done
done
echo "Results: $output (IPv4/Shadowsocks, MTU 9000, GOMAXPROCS=4)"
