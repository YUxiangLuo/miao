import yaml from "yaml";
import type { Outbound, Anytls, Hysteria2, ClashProxy } from "./types";
import sing_box_config from "./config";
import panel from "./panel/index.html";
import fs from "fs/promises";

const config = yaml.parse(await Bun.file("./miao.yaml").text());
console.log(config);
const links = config.subs;

const my_ountbounds = !config.nodes ? [] : config.nodes.map((x: string) => JSON.parse(x)) as Outbound[];
const my_names = my_ountbounds.map(x => x.tag);

const final_node_names: string[] = [];
const final_outbounds: Outbound[] = [];



const port = config.port as number;
const loc = config.loc[0] as string;
Bun.serve({
  port,
  routes: {
    "/": panel,
    "/api/config": () => {
      return new Response(Bun.file(loc));
    },
    "/api/config/status": async () => {
      const config_status = await fs.stat(loc);
      console.log(new Date(config_status.mtimeMs).toLocaleString());
      return new Response(JSON.stringify(config_status, null, 2));
    },
    "/api/go": async () => {
      for (const link of links) {
        const { node_names, outbounds } = await fetch_sub(link);
        final_node_names.push(...node_names);
        final_outbounds.push(...outbounds);
      }

      sing_box_config.outbounds[0]!.outbounds!.push(...my_names, ...final_node_names);
      sing_box_config.outbounds.push(...my_ountbounds, ...final_outbounds);
      await Bun.write(loc, JSON.stringify(sing_box_config, null, 4));
      return new Response(JSON.stringify(await fs.stat(loc)));
    }
  },
  development: true
})

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



