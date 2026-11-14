export type ToolInputSchema = { type: 'object'; properties?: Record<string, unknown>; required?: string[]; [key: string]: unknown };
export type ToolResult = string | number | boolean | null | Record<string, unknown> | {
  content: Array<{ type: string; text?: string; [key: string]: unknown }>;
  isError?: boolean;
};
export type McpHandler = (args: Record<string, unknown>, context: { requestId: unknown; toolName: string }) => ToolResult | Promise<ToolResult>;
export class DominionToolServer {
  name: string;
  version: string;
  token: string;
  maxBodyBytes: number;
  url: string | null;
  constructor(options?: { name?: string; version?: string; token?: string; maxBodyBytes?: number });
  tool(name: string, options: { description?: string; inputSchema?: ToolInputSchema }, handler: McpHandler): this;
  listen(options?: { host?: string; port?: number }): Promise<string>;
  close(): Promise<void>;
}
