import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

import { runAtlas, type AtlasRunEvent, type AtlasRunOptions } from '../index.js';
import {
  parseRuntimeTurnRequest,
  runtimeErrorEvent,
  runtimeEvent,
  serializeRuntimeMessage,
  type RuntimeEvent,
  type RuntimeTurnRequest,
} from './protocol.js';
import { UnixSocketServer } from './transport/unix/server.js';

export const DEFAULT_RUNTIME_SOCKET_PATH = '/tmp/atlas-runtime.sock';

export type RuntimeServerOptions = {
  socketPath?: string;
  runOptions?: Omit<AtlasRunOptions, 'conversationId' | 'onEvent'>;
};

export class AtlasRuntimeServer {
  public readonly socketPath: string;

  private readonly runOptions;
  private readonly transport: UnixSocketServer;
  private readonly conversationQueues = new Map<string, Promise<void>>();

  public constructor(options: RuntimeServerOptions = {}) {
    this.socketPath =
      options.socketPath ?? process.env.ATLAS_RUNTIME_SOCKET ?? DEFAULT_RUNTIME_SOCKET_PATH;
    this.runOptions = options.runOptions ?? {};
    this.transport = new UnixSocketServer({
      socketPath: this.socketPath,
      onLine: (line, send) => this.handleLine(line, send),
    });
  }

  public listen(): Promise<void> {
    return this.transport.listen();
  }

  public close(): Promise<void> {
    return this.transport.close();
  }

  private handleLine(line: string, send: (payload: string) => void): Promise<void> {
    let request: RuntimeTurnRequest;
    try {
      request = parseRuntimeTurnRequest(line);
    } catch (error) {
      send(
        serializeRuntimeMessage(
          runtimeErrorEvent(error instanceof Error ? error.message : String(error)),
        ),
      );
      return Promise.resolve();
    }

    const previous = this.conversationQueues.get(request.conversation_id) ?? Promise.resolve();
    const current = previous
      .catch(() => undefined)
      .then(() => this.handleTurn(request, send))
      .catch((error) => {
        // A fila nao pode deixar uma falha inesperada sem um evento terminal.
        send(
          serializeRuntimeMessage(
            runtimeErrorEvent(error instanceof Error ? error.message : String(error), request),
          ),
        );
      });
    this.conversationQueues.set(request.conversation_id, current);
    const clearQueue = () => {
      if (this.conversationQueues.get(request.conversation_id) === current) {
        this.conversationQueues.delete(request.conversation_id);
      }
    };
    void current.then(clearQueue, clearQueue);
    return current;
  }

  private async handleTurn(
    request: RuntimeTurnRequest,
    send: (payload: string) => void,
  ): Promise<void> {
    const publish = (message: RuntimeEvent) => {
      send(serializeRuntimeMessage(message));
    };
    publish(runtimeEvent(request, 'turn.started'));

    let messageId: string | undefined;
    try {
      const result = await runAtlas(request.input, {
        ...this.runOptions,
        conversationId: request.conversation_id,
        onEvent: (event) => {
          messageId = eventMessageId(event) ?? messageId;
          publish(atlasEvent(request, event));
        },
      });
      const content =
        typeof result.finalOutput === 'string'
          ? result.finalOutput
          : JSON.stringify(result.finalOutput);
      publish(
        runtimeEvent(request, 'turn.completed', {
          ...(messageId === undefined ? {} : { message_id: messageId }),
          content: content ?? '',
        }),
      );
    } catch (error) {
      publish(runtimeErrorEvent(error instanceof Error ? error.message : String(error), request));
    }
  }
}

function eventMessageId(event: AtlasRunEvent): string | undefined {
  return 'messageId' in event ? event.messageId : undefined;
}

function atlasEvent(request: RuntimeTurnRequest, event: AtlasRunEvent): RuntimeEvent {
  switch (event.type) {
    case 'message.delta':
      return runtimeEvent(request, 'message.delta', {
        message_id: event.messageId,
        delta: event.delta,
      });
    case 'message.completed':
      return runtimeEvent(request, 'message.completed', {
        message_id: event.messageId,
        content: event.content,
      });
    case 'tool.started':
      return runtimeEvent(request, 'tool.started', {
        tool_id: event.toolId,
        name: event.toolName,
      });
    case 'tool.completed':
      return runtimeEvent(request, 'tool.completed', {
        tool_id: event.toolId,
        name: event.toolName,
        ...(event.output === undefined ? {} : { output: event.output }),
      });
  }
}

function isMainModule(): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && import.meta.url === pathToFileURL(resolve(entrypoint)).href;
}

if (isMainModule()) {
  const server = new AtlasRuntimeServer();
  await server.listen();
  console.log(`Atlas Runtime listening on ${server.socketPath}`);

  const shutdown = async () => {
    await server.close();
    process.exit(0);
  };
  process.once('SIGINT', shutdown);
  process.once('SIGTERM', shutdown);
}
