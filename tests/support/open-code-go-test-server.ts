import { createServer, type IncomingHttpHeaders } from 'node:http';

export type CapturedOpenCodeGoRequest = {
  body: Record<string, unknown>;
  headers: IncomingHttpHeaders;
  url: string | undefined;
};

export type OpenCodeGoTestServer = {
  baseURL: string;
  requests: CapturedOpenCodeGoRequest[];
  close: () => Promise<void>;
};

export async function startOpenCodeGoTestServer(
  responses: readonly string[] = [],
): Promise<OpenCodeGoTestServer> {
  const requests: CapturedOpenCodeGoRequest[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }

    requests.push({
      body: JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>,
      headers: request.headers,
      url: request.url,
    });

    const responseText = responses[requests.length - 1] ?? `Test response ${requests.length}`;

    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(
      JSON.stringify({
        id: `test-response-${requests.length}`,
        object: 'response',
        created_at: 1,
        status: 'completed',
        model: 'gpt-5.6-luna',
        output: [
          {
            id: `test-message-${requests.length}`,
            type: 'message',
            status: 'completed',
            role: 'assistant',
            content: [{ type: 'output_text', text: responseText, annotations: [] }],
          },
        ],
        usage: {
          input_tokens: 1,
          output_tokens: 1,
          total_tokens: 2,
        },
      }),
    );
  });

  const port = await new Promise<number>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('OpenCode Go test server did not receive a TCP address.'));
        return;
      }

      resolve(address.port);
    });
  });

  return {
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    requests,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      }),
  };
}
