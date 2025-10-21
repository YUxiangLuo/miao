import type { Outbound, Anytls, Hysteria2, ClashProxy } from "./types";
import yaml from "yaml";
import get_config from "./template-config";
import fs from "fs/promises";
import { gen_direct } from "./rule";

// 从配置文件获取基础配置
const config = yaml.parse(await Bun.file("./miao.yaml").text());
const port = config.port as number;
const sing_box_home = config.sing_box_home as string;
const config_output_loc = sing_box_home + "/config.json";
const subs = config.subs as string[];
const nodes = (config.nodes || []) as string[];

await gen_config();
let sing_process: Bun.Subprocess | null;
// await start_sing();
Bun.serve({
  port,
  routes: {
    "/api/rule/generate": async () => {
      try {
        await gen_direct();
        return new Response(
          JSON.stringify(await fs.stat(sing_box_home + "/chinasite.srs")),
        );
      } catch (error) {
        console.log(error);
        return new Response("rule generation failed", { status: 500 });
      }
    },
    "/api/config": async () => {
      try {
        const config_stat = await fs.stat(config_output_loc);
        const config_content = JSON.stringify(
          JSON.parse(await Bun.file(config_output_loc).text()),
          null,
          2,
        );
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
    "/api/config/generate": async () => {
      try {
        await gen_config();
        return new Response(Bun.file(config_output_loc));
      } catch (error) {
        console.log(error);
        return new Response("500", { status: 500 });
      }
    },
    "/api/sing/log-live": async () => {
      if (!sing_process) return new Response("not running", { status: 404 });
      if (sing_process.killed)
        return new Response("not running", { status: 404 });
      return new Response(
        (await Bun.$`tail -n 50 ${sing_box_home}/box.log`).text(),
      );
    },
    "/api/sing/restart": async (req: Request) => {
      try {
        stop_sing();
        await start_sing();
        return new Response("ok");
      } catch (error) {
        console.log(error);
        return new Response(String(error), { status: 500 });
      }
    },
    "/api/sing/start": async (req: Request) => {
      if (!sing_process || (sing_process && sing_process.killed)) {
        try {
          await start_sing();
          return new Response("ok");
        } catch (error) {
          console.log(error);
          return new Response("error", { status: 500 });
        }
      } else {
        return new Response("sing box is already running", { status: 500 });
      }
    },
    "/api/sing/stop": async () => {
      stop_sing();
      return new Response("stopped");
    },
    "/api/net-checks/manual": async () => {
      if (await check_connection()) return new Response("ok");
      else return new Response("not ok", { status: 500 });
    },
  },
  development: false,
});

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
}

async function gen_config() {
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
  console.log(sing_box_config);
  await Bun.write(config_output_loc, JSON.stringify(sing_box_config, null, 4));
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
  try {
    const res = Bun.$`curl -I https://gstatic.com/generate_204`;
    const res_text = await res.text();
    if (res_text.includes("HTTP/2 204")) {
      return true;
    } else {
      return false;
    }
  } catch (error) {
    return false;
  }
}
