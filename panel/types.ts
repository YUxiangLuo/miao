export type FileStat = {
  atimeMs: number;
  mtimeMs: number;
  ctimeMs: number;
  birthtimeMs: number;
  size: number;
};

export type Config = {
  config_stat: FileStat;
  config_content: string;
};
