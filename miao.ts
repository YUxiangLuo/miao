import yaml from "yaml";
import type { Outbound, Anytls, Hysteria2, ClashProxy } from "./types";
import get_config from "./config";
import panel from "./panel/index.html";
import fs from "fs/promises";
import db from "./db";


const config = yaml.parse(await Bun.file("./miao.yaml").text());
const port = config.port as number;
const sing_box_home = config.sing_box_home as string;
const loc = config.loc[0] as string;
const subs = config.subs as string[]
const nodes = (config.nodes || []) as string[];

function sing() {
  const p = Bun.spawn({
    cwd: sing_box_home,
    cmd: ["sing-box", "run", "-c", "config.json"],
    env: { ...Bun.env, PATH: `${Bun.env.PATH}:${sing_box_home}` },
    stdout: "inherit",
    stderr: "inherit"
  })
  return String(p.pid);
}

if (!(Bun.env.IS_ARCH === "true")) {
  await gen_config(subs, nodes);
  const pid = sing();
  Bun.write("./pid.sing", pid);
}

setInterval(async () => {
  const time = new Date().toISOString();
  try {
    const res = await Bun.$`curl -I https://gstatic.com/generate_204`;
    const res_text = await res.text();
    if (res_text.includes("HTTP/2 204")) {
      db.prepare(`insert into checks (status, time) values (1, '${time}');`).run();
    } else {
      db.prepare(`insert into checks (status, time) values (0, '${time}');`).run();
    }
  } catch (error) {
    db.prepare(`insert into checks (status, time) values (0, '${time}');`).run();
  }
}, 5 * 60 * 1000);


Bun.serve({
  port,
  routes: {
    "/": panel,
    "/api/config": () => {
      return new Response(Bun.file(loc));
    },
    "/api/config/status": async () => {
      const config_status = await fs.stat(loc);
      return new Response(JSON.stringify(config_status, null, 2));
    },
    "/api/config/generate": async () => {
      await gen_config(subs, nodes);
      return new Response(JSON.stringify(await fs.stat(loc)));
    },
    "/api/sing/restart": async () => {
      await Bun.$`killall sing-box && sleep 2`.nothrow();
      const pid = sing();
      return new Response(String(pid));
    },
    "/api/checks": async() => {
      const res = db.prepare("select * from checks order by id DESC limit 10;").all();
      return new Response(JSON.stringify(res));
    }
  },
  development: true
})

async function gen_config(subs: string[], nodes: string[]) {
  const my_ountbounds = nodes.map((x: string) => JSON.parse(x)) as Outbound[];
  const my_names = my_ountbounds.map(x => x.tag);

  const final_outbounds: Outbound[] = [];
  const final_node_names: string[] = [];
  for (const sub of subs) {
    const { node_names, outbounds } = await fetch_sub(sub);
    final_node_names.push(...node_names);
    final_outbounds.push(...outbounds);
  }
  const sing_box_config = get_config();
  sing_box_config.outbounds[0]!.outbounds!.push(...my_names, ...final_node_names);
  sing_box_config.outbounds.push(...my_ountbounds, ...final_outbounds);
  await Bun.write(loc, JSON.stringify(sing_box_config, null, 4));
}

async function fetch_sub(link: string) {
  const res_body_text = await (await fetch(link, { headers: { "User-Agent": "clash-meta" } })).text();
  const clash_obj = yaml.parse(res_body_text);
  const nodes = clash_obj.proxies.filter((x: any) => (x.name.includes("JP") || x.name.includes("TW") || x.name.includes("SG"))) as ClashProxy[];
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



