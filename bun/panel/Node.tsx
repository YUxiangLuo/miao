import type { Node } from "./types";
export function NodeCard({ node }: { node: Node }) {
  return (
    <div className="flex flex-col p-4 min-w-48 border-1 shadow rounded-md bg-background text-foreground">
      <span className="text-lg font-bold">{node.tag}</span>
      <span>{node.type}</span>
    </div>
  );
}

export function NodeList({ nodes }: { nodes: Node[] }) {
  return (
    <div className="flex gap-4 flex-wrap">
      {nodes.map((node) => (
        <NodeCard key={node.tag} node={node} />
      ))}
    </div>
  );
}
