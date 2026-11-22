import { build, type Plugin } from "vite";

const QUERY = "?frame-script";

export function frameScriptPlugin(): Plugin {
  return {
    name: "frame-script",
    enforce: "pre",
    async resolveId(source, importer) {
      if (!source.endsWith(QUERY)) return null;
      const entry = await this.resolve(
        source.slice(0, -QUERY.length),
        importer,
        { skipSelf: true },
      );
      return entry && entry.id + QUERY;
    },
    async load(id) {
      if (!id.endsWith(QUERY)) return null;
      const entry = id.slice(0, -QUERY.length);
      const built = await build({
        configFile: false,
        logLevel: "silent",
        publicDir: false,
        build: {
          write: false,
          minify: true,
          lib: { entry, formats: ["iife"], name: "nitFrame" },
        },
      });
      const chunk = (Array.isArray(built) ? built : [built])
        .flatMap((result) => ("output" in result ? result.output : []))
        .find((output) => output.type === "chunk");
      if (chunk?.type !== "chunk") throw new Error(`${entry} built no script`);
      for (const file of chunk.moduleIds) this.addWatchFile(file);
      return `export default ${JSON.stringify(chunk.code)};`;
    },
  };
}
