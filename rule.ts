import type { DomainSet } from "./types";
import yaml from "yaml";
import fs from "fs";
const config = yaml.parse(await Bun.file("./miao.yaml").text());
const sing_box_home = config.sing_box_home as string;
const direct_sites_link = config.rules.direct_txt as string;

export async function gen_direct() {
  try {
    const res = await fetch(direct_sites_link);
    if (!res.ok)
      throw new Error(
        `Failed to fetch direct sites: ${res.status} ${res.statusText}`,
      );
    await Bun.write(sing_box_home + "/direct.txt", await res.text());
  } catch (error) {
    console.error("Failed to fetch direct sites:", error);
    throw error;
  }

  const direct_items = (
    await Bun.file(sing_box_home + "/direct.txt").text()
  ).split("\n");

  const direct_set: DomainSet = {
    rules: [
      {
        domain: [],
        domain_suffix: [],
        domain_regex: [],
      },
    ],
    version: 3,
  };

  for (const item of direct_items) {
    if (item.startsWith("full:")) {
      direct_set.rules[0].domain.push(item.replace("full:", ""));
    } else if (item.startsWith("regexp:")) {
      direct_set.rules[0].domain_regex.push(item.replace("regexp:", ""));
    } else {
      if (item) direct_set.rules[0].domain_suffix.push(item);
    }
  }
  await Bun.write(sing_box_home + "/direct.json", JSON.stringify(direct_set));
  if (fs.existsSync(sing_box_home + "/chinasite.srs")) {
    fs.copyFileSync(
      sing_box_home + "/chinasite.srs",
      sing_box_home + "/chinasite.srs.bak",
    );
  }
  const p = Bun.spawn({
    cwd: sing_box_home,
    cmd: [
      "sing-box",
      "rule-set",
      "compile",
      "--output",
      sing_box_home + "/chinasite.srs",
      sing_box_home + "/direct.json",
    ],
    env: {
      ...Bun.env,
      PATH: `${Bun.env.PATH}:${sing_box_home}`,
    },
    stdout: "inherit",
    stderr: "inherit",
  });
  await Bun.sleep(1000);
  if (p.exitCode !== 0) {
    console.error("Failed to compile rule set");
    if (fs.existsSync(sing_box_home + "/chinasite.srs"))
      fs.unlinkSync(sing_box_home + "/chinasite.srs");
    if (fs.existsSync(sing_box_home + "/chinasite.srs.bak"))
      fs.copyFileSync(
        sing_box_home + "/chinasite.srs.bak",
        sing_box_home + "/chinasite.srs",
      );
    throw new Error("Failed to compile rule set");
  }
}
