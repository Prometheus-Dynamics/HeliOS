import type { Node } from '@xyflow/svelte';
import type { XYPosition } from '@xyflow/system';

export function findNodeAtFlowPosition(nodes: Node[], position: XYPosition): Node | null {
  const matches: Node[] = [];
  nodes.forEach((node) => {
    const nodeX = node.position?.x ?? 0;
    const nodeY = node.position?.y ?? 0;
    const width = node.measured?.width ?? node.width ?? 240;
    const height = node.measured?.height ?? node.height ?? 140;
    if (
      position.x >= nodeX &&
      position.x <= nodeX + width &&
      position.y >= nodeY &&
      position.y <= nodeY + height
    ) {
      matches.push(node);
    }
  });
  return matches.length > 0 ? matches[matches.length - 1] ?? null : null;
}
