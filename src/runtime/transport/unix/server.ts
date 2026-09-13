import { createServer, type Socket } from 'node:net';
import { rm } from 'node:fs/promises';

export type UnixSocketLineHandler = (
  line: string,
  send: (payload: string) => void,
) => void | Promise<void>;

export type UnixSocketServerOptions = {
  socketPath: string;
  onLine: UnixSocketLineHandler;
};

// O transporte apenas enquadra linhas JSON e nao conhece o protocolo do Runtime.
export class UnixSocketServer {
  private readonly socketPath: string;
  private readonly onLine: UnixSocketLineHandler;
  private readonly server;
  private readonly sockets = new Set<Socket>();

  public constructor(options: UnixSocketServerOptions) {
    this.socketPath = options.socketPath;
    this.onLine = options.onLine;
    this.server = createServer((socket) => this.handleConnection(socket));
  }

  public listen(): Promise<void> {
    return new Promise((resolveListen, rejectListen) => {
      const onError = (error: Error) => {
        this.server.off('listening', onListening);
        rejectListen(error);
      };
      const onListening = () => {
        this.server.off('error', onError);
        resolveListen();
      };

      this.server.once('error', onError);
      this.server.once('listening', onListening);
      this.server.listen(this.socketPath);
    });
  }

  public async close(): Promise<void> {
    for (const socket of this.sockets) {
      socket.destroy();
    }
    await new Promise<void>((resolveClose, rejectClose) => {
      if (!this.server.listening) {
        resolveClose();
        return;
      }

      this.server.close((error) => (error ? rejectClose(error) : resolveClose()));
    });
    await rm(this.socketPath, { force: true });
  }

  private handleConnection(socket: Socket): void {
    let buffer = '';
    this.sockets.add(socket);
    socket.setEncoding('utf8');
    socket.once('close', () => this.sockets.delete(socket));
    socket.on('error', () => undefined);

    const send = (payload: string) => {
      if (!socket.destroyed) {
        socket.write(payload);
      }
    };

    socket.on('data', (chunk: string) => {
      buffer += chunk;
      let newlineIndex = buffer.indexOf('\n');
      while (newlineIndex !== -1) {
        const line = buffer.slice(0, newlineIndex).replace(/\r$/, '');
        buffer = buffer.slice(newlineIndex + 1);
        if (line.trim()) {
          void Promise.resolve(this.onLine(line, send)).catch(() => {
            // O protocolo transforma falhas em eventos; o transporte nao decide seu formato.
            socket.destroy();
          });
        }
        newlineIndex = buffer.indexOf('\n');
      }
    });
  }
}
