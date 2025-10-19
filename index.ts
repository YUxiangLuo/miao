import type { Outbound, Anytls, Hysteria2, ClashProxy } from "./types";
import yaml from "yaml";
import get_config from "./config";
import fs from "fs/promises";
import db from "./db";


// 从配置文件获取基础配置
const config = yaml.parse(await Bun.file("./miao.yaml").text());
const port = config.port as number;
const sing_box_home = config.sing_box_home as string;
const loc = sing_box_home + "/config.json";
const subs = config.subs as string[];
const nodes = (config.nodes || []) as string[];


await gen_config(subs, nodes);
// 全局共享sing-box进程
let sing_process: Bun.Subprocess | null;
await start_sing();
setInterval(check_connection, 1 * 60 * 1000);

async function start_sing() {
  if (sing_process && !sing_process.killed) throw Error("already running!");
  sing_process = Bun.spawn({
    cwd: sing_box_home,
    cmd: ["sing-box", "run", "-c", "config.json"],
    env: { ...Bun.env, PATH: `${Bun.env.PATH}:${sing_box_home}` },
    stdout: "ignore",
    stderr: "ignore",
  });
  await Bun.sleep(3000);
  if (sing_process.exitCode !== null) {
    sing_process = null;
    throw Error("sing box failed to start");
  }
  if (await check_connection()) {
    await Bun.write(sing_box_home + "/pid", String(sing_process.pid));
    record_sing(1);
    return sing_process.pid;
  } else {
    sing_process.kill(9);
    sing_process = null;
    throw Error("sing box started but failed to connect to internet");
  }
}

function stop_sing() {
  if (!sing_process) return;
  if (sing_process.killed) return;
  sing_process.kill(9);
  sing_process = null;
  record_sing(0);
}


Bun.serve({
  port,
  routes: {
    "/api/checks": async (req: Request) => {
      const checks = db
        .query("select * from checks order by id desc limit 10;")
        .all();
      return new Response(JSON.stringify(checks, null, 2), {
        headers: {
          "Content-Type": "application/json",
        },
      });
    },
    "/api/config": async (req: Request) => {
      try {
        const config_stat = await fs.stat(loc);
        const config_content = await Bun.file(loc).text();
        return new Response(
          JSON.stringify({
            config_stat,
            config_content,
          }),
        );
      } catch (error) {
        return new Response("config file not found", { status: 404 });
      }
    },
    "/api/config/generate": async (req: Request) => {
      try {
        await gen_config(subs, nodes);
        return new Response(Bun.file(loc));
      } catch (error) {
        console.log(error);
        return new Response("500", { status: 500 });
      }
    },
    "/api/sing/log-live": async (req: Request) => {
      if (!sing_process) return new Response("not running", { status: 404 });
      if (sing_process.killed)
        return new Response("not running", { status: 404 });
      return new Response(
        (await Bun.$`tail -n 50 ${sing_box_home}/box.log`).text(),
      );
    },
    "/api/sing/status": async (req: Request) => {
      const running = !!(sing_process && !sing_process.killed);
      return new Response(JSON.stringify({ running }), {
        headers: { "Content-Type": "application/json" },
      });
    },
    "/api/sing/action-records": async (req: Request) => {
      const action_records = db
        .query("select * from sing_record order by id desc limit 10;")
        .all();
      return new Response(JSON.stringify(action_records, null, 2), {
        headers: {
          "Content-Type": "application/json",
        },
      });
    },
    "/api/sing/restart": async (req: Request) => {
      try {
        stop_sing();
        await start_sing();
        return new Response("200");
      } catch (error) {
        console.log(error);
        return new Response(String(error), { status: 500 });
      }
    },
    "/api/sing/start": async (req: Request) => {
      if (!sing_process || (sing_process && sing_process.killed)) {
        try {
          await start_sing();
          return new Response("200");
        } catch (error) {
          console.log(error);
          return new Response("500", { status: 500 });
        }
      } else {
        return new Response("sing box is already running", { status: 500 });
      }
    },
    "/api/sing/stop": async (req: Request) => {
      stop_sing();
      return new Response("stopped");
    },
    "/api/net-checks/manual": async (req: Request) => {
      try {
        await check_connection();
        const checks = db
          .query("select * from checks order by id desc limit 10;")
          .all();
        return new Response(JSON.stringify(checks, null, 2), {
          headers: {
            "Content-Type": "application/json",
          },
        });
      } catch (error) {
        console.log(error);
        return new Response("500", { status: 500 });
      }
    },
  },
  development: false,
});

function record_sing(action: number) {
  try {
    const time = new Date().toISOString();
    db.prepare(
      `insert into sing_record (type, time) values (${action}, '${time}');`,
    ).run();
  } catch (error) {
    throw error;
  }
}

async function gen_config(subs: string[], nodes: string[]) {
  const my_ountbounds = nodes.map((x: string) => JSON.parse(x)) as Outbound[];
  const my_names = my_ountbounds.map((x) => x.tag);

  const final_outbounds: Outbound[] = [];
  const final_node_names: string[] = [];
  for (const sub of subs) {
    const { node_names, outbounds } = await fetch_sub(sub);
    final_node_names.push(...node_names);
    final_outbounds.push(...outbounds);
  }
  const sing_box_config = get_config();
  sing_box_config.outbounds[0]!.outbounds!.push(
    ...my_names,
    ...final_node_names,
  );
  sing_box_config.outbounds.push(...my_ountbounds, ...final_outbounds);
  await Bun.write(loc, JSON.stringify(sing_box_config, null, 4));
}

async function fetch_sub(link: string) {
  let clash_obj;
  const res_body_text = await (
    await fetch(link, { headers: { "User-Agent": "clash-meta" } })
  ).text();
  clash_obj = yaml.parse(res_body_text);

  const nodes = clash_obj.proxies.filter(
    (x: any) =>
      x.name.includes("JP") || x.name.includes("TW") || x.name.includes("SG"),
  ) as ClashProxy[];
  const node_names = [];

  const outbounds: Outbound[] = [];
  for (const node of nodes) {
    switch (node.type) {
      case "anytls":
        const anytls_outbound: Anytls = {
          tag: node.name,
          type: node.type,
          server: node.server,
          server_port: node.port,
          password: node.password,
          tls: {
            enabled: true,
            insecure: node["skip-cert-verify"],
            server_name: node.sni,
          },
        };
        node_names.push(node.name);
        outbounds.push(anytls_outbound);
        break;
      case "hysteria2":
        const hysteria2_outbound: Hysteria2 = {
          tag: node.name,
          type: node.type,
          server: node.server,
          server_port: node.port,
          password: node.password,
          up_mbps: 40,
          down_mbps: 350,
          tls: {
            enabled: true,
            insecure: true,
            server_name: node.sni,
          },
        };
        node_names.push(node.name);
        outbounds.push(hysteria2_outbound);
        break;
      default:
        break;
    }
  }
  return { node_names, outbounds };
}

async function check_connection(): Promise<boolean> {
  const time = new Date().toISOString();
  try {
    const res = Bun.$`curl -I https://gstatic.com/generate_204`;
    const res_text = await res.text();
    if (res_text.includes("HTTP/2 204")) {
      db.prepare(
        `insert into checks (status, time) values (1, '${time}');`,
      ).run();
      return true;
    } else {
      db.prepare(
        `insert into checks (status, time) values (0, '${time}');`,
      ).run();
      return false;
    }
  } catch (error) {
    db.prepare(
      `insert into checks (status, time) values (0, '${time}');`,
    ).run();
    return false;
  }
}
