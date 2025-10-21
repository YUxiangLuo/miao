// net-check.ts
// Bun 1.1+ / TypeScript 5+
// 提供网络状态体检并以 NDJSON（每行一个 JSON）流式输出。

/** 简单 Writer 接口，用于向任意“可写目标”写入字符串（比如 HTTP Response 的 readable controller） */
export type Writer = { write: (s: string) => void };

/** 体检可选项 */
export interface NetworkCheckOptions {
  /** 指定要 ping 的目标；若未指定，将自动使用默认网关和 8.8.8.8 */
  pingHosts?: string[];
  /** 要解析的域名列表 */
  dnsTests?: string[];
  /** 是否输出防火墙规则（nft/iptables） */
  checkFirewall?: boolean;
  /** 是否输出监听端口/统计（ss/netstat） */
  listeningPorts?: boolean;
  /** 单条命令超时时间（毫秒） */
  timeoutMsPerCmd?: number;
}

/** 默认选项 */
const DEFAULT_OPTS: Required<Omit<NetworkCheckOptions, "pingHosts">> = {
  dnsTests: ["google.com", "cloudflare.com"],
  checkFirewall: true,
  listeningPorts: true,
  timeoutMsPerCmd: 15000,
};

/** 把一行对象写为 NDJSON */
function writeLine(writer: Writer, obj: Record<string, unknown>) {
  writer.write(JSON.stringify({ ts: new Date().toISOString(), ...obj }) + "\n");
}

/** 检查命令是否存在（sh -c 'command -v ...'） */
async function hasCmd(cmd: string): Promise<boolean> {
  const p = Bun.spawn(["sh", "-c", `command -v ${cmd} >/dev/null 2>&1`], {
    stdout: "ignore",
    stderr: "ignore",
  });
  const code = await p.exited;
  return code === 0;
}

/** 逐行读取 ReadableStream 并回调 */
async function readStreamLines(
  stream: ReadableStream<Uint8Array>,
  onLine: (line: string) => void,
) {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let idx: number;
    while ((idx = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, idx).replace(/\r$/, "");
      buf = buf.slice(idx + 1);
      if (line) onLine(line);
    }
  }
  if (buf) onLine(buf);
}

/** 运行外部命令：边读 stdout/stderr 边写入 NDJSON，带超时与 Abort 控制 */
async function runCmd(
  writer: Writer,
  section: string,
  cmd: string,
  args: string[] = [],
  timeoutMs = 15000,
): Promise<void> {
  if (!(await hasCmd(cmd))) {
    writeLine(writer, {
      section,
      level: "warn",
      msg: `command not found: ${cmd}`,
    });
    return;
  }

  writeLine(writer, {
    section,
    level: "info",
    msg: `running: ${cmd} ${args.join(" ")}`,
  });

  const ac = new AbortController();
  const timer = setTimeout(() => {
    writeLine(writer, {
      section,
      level: "warn",
      msg: `timeout after ${timeoutMs}ms: ${cmd}`,
    });
    ac.abort("timeout");
  }, timeoutMs);

  const proc = Bun.spawn([cmd, ...args], {
    stdout: "pipe",
    stderr: "pipe",
    signal: ac.signal,
  });

  const stdoutDone = proc.stdout
    ? readStreamLines(proc.stdout, (line) =>
        writeLine(writer, { section, level: "data", stream: "stdout", line }),
      )
    : Promise.resolve();
  const stderrDone = proc.stderr
    ? readStreamLines(proc.stderr, (line) =>
        writeLine(writer, { section, level: "data", stream: "stderr", line }),
      )
    : Promise.resolve();

  try {
    await proc.exited;
  } catch {
    // 可能是超时 abort 导致
  } finally {
    clearTimeout(timer);
    await Promise.all([stdoutDone, stderrDone]);
  }
}

/** 执行一条 shell 并捕获输出（用于解析默认网关等） */
async function shCapture(cmd: string): Promise<string> {
  const p = Bun.spawn(["sh", "-c", cmd], { stdout: "pipe", stderr: "pipe" });
  const out = await new Response(p.stdout!).text();
  await p.exited;
  return out;
}

/** 解析默认网关（支持 IPv4/IPv6） */
async function getDefaultGateway(): Promise<string | null> {
  try {
    const out = await shCapture("ip route show default | head -n1");
    // 优先 IPv4
    const m4 = out.match(/default via ([0-9.]+)/);
    if (m4) return m4[1]!;
    // 再 IPv6
    const m6 = out.match(/default via ([0-9a-fA-F:]+)/);
    return m6 ? m6[1]! : null;
  } catch {
    return null;
  }
}

/** 读取 resolv.conf 的 nameserver */
async function getNameServers(): Promise<string[]> {
  try {
    const txt = await Bun.file("/etc/resolv.conf").text();
    return [...txt.matchAll(/^\s*nameserver\s+([^\s#]+)/gm)].map((m) => m[1]!);
  } catch {
    return [];
  }
}

/**
 * 主函数：流式网络体检
 * - 将各阶段/命令的输出以 NDJSON 行写入 `writer`
 */
export async function streamNetworkCheck(
  writer: Writer,
  options: NetworkCheckOptions = {},
) {
  const opts = { ...DEFAULT_OPTS, ...options };

  writeLine(writer, {
    section: "start",
    level: "info",
    msg: "network check started",
  });

  // 1) 链路/地址/路由/邻居
  await runCmd(
    writer,
    "link",
    "ip",
    ["-details", "link", "show"],
    opts.timeoutMsPerCmd,
  );
  await runCmd(
    writer,
    "addr",
    "ip",
    ["-details", "addr", "show"],
    opts.timeoutMsPerCmd,
  );
  await runCmd(
    writer,
    "route",
    "ip",
    ["route", "show", "table", "main"],
    opts.timeoutMsPerCmd,
  );
  await runCmd(writer, "neigh", "ip", ["neigh", "show"], opts.timeoutMsPerCmd);

  // 2) 默认网关
  const gw = await getDefaultGateway();
  if (gw)
    writeLine(writer, {
      section: "route",
      level: "info",
      msg: `default gw: ${gw}`,
    });

  // 3) 连通性：ping（优先系统 ping，降级 busybox）
  const targets = (
    opts.pingHosts && opts.pingHosts.length ? opts.pingHosts : [gw, "8.8.8.8"]
  ).filter(Boolean) as string[];
  for (const host of targets) {
    if (await hasCmd("ping")) {
      await runCmd(writer, "ping", "ping", ["-c", "3", "-n", host], 8000);
    } else if (await hasCmd("busybox")) {
      await runCmd(writer, "ping", "busybox", ["ping", "-c", "3", host], 8000);
    } else {
      writeLine(writer, {
        section: "ping",
        level: "warn",
        msg: "no ping available",
      });
      break;
    }
  }

  // 4) DNS：resolv.conf / dig / nslookup / getent
  const resolvers = await getNameServers();
  writeLine(writer, {
    section: "dns",
    level: "info",
    msg: `nameservers: ${resolvers.join(", ") || "(none)"}`,
  });

  if (await hasCmd("dig")) {
    for (const name of opts.dnsTests) {
      await runCmd(
        writer,
        "dns",
        "dig",
        ["+time=3", "+tries=1", name, "A", "@8.8.8.8"],
        5000,
      );
    }
  } else if (await hasCmd("nslookup")) {
    for (const name of opts.dnsTests) {
      await runCmd(writer, "dns", "nslookup", [name, "8.8.8.8"], 5000);
    }
  } else if (await hasCmd("getent")) {
    for (const name of opts.dnsTests) {
      await runCmd(writer, "dns", "getent", ["hosts", name], 5000);
    }
  } else {
    writeLine(writer, {
      section: "dns",
      level: "warn",
      msg: "no dig/nslookup/getent; skip DNS tests",
    });
  }

  // 5) 端口/服务
  if (opts.listeningPorts) {
    if (await hasCmd("ss")) {
      await runCmd(writer, "ports", "ss", ["-tupln"], opts.timeoutMsPerCmd);
      await runCmd(writer, "ports", "ss", ["-s"], opts.timeoutMsPerCmd);
    } else if (await hasCmd("netstat")) {
      await runCmd(
        writer,
        "ports",
        "netstat",
        ["-tupln"],
        opts.timeoutMsPerCmd,
      );
    } else {
      writeLine(writer, {
        section: "ports",
        level: "warn",
        msg: "neither ss nor netstat found",
      });
    }
  }

  // 6) 防火墙
  if (opts.checkFirewall) {
    if (await hasCmd("nft")) {
      await runCmd(
        writer,
        "firewall",
        "nft",
        ["list", "ruleset"],
        opts.timeoutMsPerCmd,
      );
    } else if (await hasCmd("iptables")) {
      await runCmd(
        writer,
        "firewall",
        "iptables",
        ["-L", "-v", "-n"],
        opts.timeoutMsPerCmd,
      );
      if (await hasCmd("ip6tables")) {
        await runCmd(
          writer,
          "firewall",
          "ip6tables",
          ["-L", "-v", "-n"],
          opts.timeoutMsPerCmd,
        );
      }
    } else {
      writeLine(writer, {
        section: "firewall",
        level: "warn",
        msg: "no nft/iptables found",
      });
    }
  }

  writeLine(writer, {
    section: "done",
    level: "info",
    msg: "network check finished",
  });
}

/**
 * 工具：创建一个 ReadableStream（NDJSON），便于直接用在 `new Response(stream)`。
 * 适合你的路由里直接 return。
 */
export function createNetworkCheckReadableStream(
  options?: NetworkCheckOptions,
): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start: async (controller) => {
      const enc = new TextEncoder();
      const writer: Writer = {
        write: (s: string) => controller.enqueue(enc.encode(s)),
      };
      try {
        await streamNetworkCheck(writer, options);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        controller.enqueue(
          enc.encode(
            JSON.stringify({
              ts: new Date().toISOString(),
              section: "error",
              level: "error",
              msg,
            }) + "\n",
          ),
        );
      } finally {
        controller.close();
      }
    },
  });
}

/** 可选：命令行运行（便于本地调试） */
if (import.meta.path === Bun.main) {
  const pingHosts = process.env.PING_HOSTS?.split(",").filter(Boolean);
  const dnsTests = process.env.DNS_TESTS?.split(",").filter(Boolean);
  const checkFirewall = process.env.FIREWALL === "0" ? false : true;
  const listeningPorts = process.env.PORTS === "0" ? false : true;

  // 直接写 stdout
  const stdoutWriter: Writer = {
    write: (s) => Bun.write(Bun.stdout, s),
  };

  await streamNetworkCheck(stdoutWriter, {
    pingHosts,
    dnsTests,
    checkFirewall,
    listeningPorts,
  });
}
