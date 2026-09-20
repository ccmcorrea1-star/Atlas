import { createServer } from 'node:http';

type WireMessage = Record<string, unknown>;

export type StreamingTestServer = {
  baseURL: string;
  requests: WireMessage[];
  close: () => Promise<void>;
};

function responseBody(status: string, output: WireMessage[] = []): WireMessage {
  return {
    id: 'streaming-test-response',
    object: 'response',
    created_at: 1,
    status,
    model: 'gpt-5.6-luna',
    output,
    usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 },
  };
}

export async function startStreamingTestServer(
  responseText = 'Alteração verificada.',
): Promise<StreamingTestServer> {
  const requests: WireMessage[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }
    requests.push(JSON.parse(Buffer.concat(chunks).toString('utf8')) as WireMessage);

    const message = {
      id: 'streaming-test-message',
      type: 'message',
      status: 'completed',
      role: 'assistant',
      content: [{ type: 'output_text', text: responseText, annotations: [] }],
    };
    if (requests.at(-1)?.stream !== true) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify(responseBody('completed', [message])));
      return;
    }
    const events = [
      { type: 'response.created', sequence_number: 1, response: responseBody('in_progress') },
      {
        type: 'response.output_item.added',
        sequence_number: 2,
        output_index: 0,
        item: {
          id: message.id,
          type: 'message',
          status: 'in_progress',
          role: 'assistant',
          content: [],
        },
      },
      {
        type: 'response.content_part.added',
        sequence_number: 3,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        part: { type: 'output_text', text: '', annotations: [] },
      },
      {
        type: 'response.output_text.delta',
        sequence_number: 4,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        delta: responseText,
      },
      {
        type: 'response.output_text.done',
        sequence_number: 5,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        text: responseText,
      },
      {
        type: 'response.content_part.done',
        sequence_number: 6,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        part: message.content[0],
      },
      { type: 'response.output_item.done', sequence_number: 7, output_index: 0, item: message },
      {
        type: 'response.completed',
        sequence_number: 8,
        response: responseBody('completed', [message]),
      },
    ];

    response.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of events) {
      response.write(`data: ${JSON.stringify(event)}\n\n`);
    }
    response.end();
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Streaming test server did not receive a TCP address.'));
        return;
      }
      resolvePort(address.port);
    });
  });

  return {
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    requests,
    close: () =>
      new Promise<void>((resolveClose, rejectClose) => {
        server.close((error) => (error ? rejectClose(error) : resolveClose()));
      }),
  };
}
