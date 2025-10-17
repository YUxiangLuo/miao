import yaml from "yaml";
import type { Outbound, Anytls, Hysteria2, ClashProxy } from "./types";
import sing_box_config from "./config";

const links_and_nodes =  yaml.parse(await Bun.file("./miao.yaml").text());
const links = links_and_nodes.subs;

const my_ountbounds = !links_and_nodes.nodes?[]:links_and_nodes.nodes.map((x: string) => JSON.parse(x)) as Outbound[];
const my_names = my_ountbounds.map(x => x.tag);

const final_node_names: string[] = [];
const final_outbounds:  Outbound[] = [];


for(const link of links) {
  const {node_names, outbounds} =  await fetch_sub(link);
  final_node_names.push(...node_names);
  final_outbounds.push(...outbounds);
}

sing_box_config.outbounds[0]!.outbounds!.push(...my_names, ...final_node_names);
sing_box_config.outbounds.push(...my_ountbounds, ...final_outbounds);
await Bun.write("./config.json", JSON.stringify(sing_box_config, null, 4));

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



