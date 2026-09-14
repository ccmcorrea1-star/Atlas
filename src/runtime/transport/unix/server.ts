import { createServer, type Socket } from 'node:net';
import { chmod, mkdir, rm } from 'node:fs/promises';
import { dirname } from 'node:path';

const MAX_RUNTIME_LINE_BYTES = 8 * 1024 * 1024;

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

  public async listen(): Promise<void> {
    await mkdir(dirname(this.socketPath), { recursive: true, mode: 0o700 });
    await new Promise<void>((resolveListen, rejectListen) => {
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
    try {
      // O socket carrega capacidades locais e deve ser acessível somente pelo usuário do Runtime.
      await chmod(this.socketPath, 0o600);
    } catch (error) {
      await this.close();
      throw error;
    }
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
        if (Buffer.byteLength(line, 'utf8') + 1 > MAX_RUNTIME_LINE_BYTES) {
          socket.destroy();
          return;
        }
        if (line.trim()) {
          void Promise.resolve(this.onLine(line, send)).catch(() => {
            // O protocolo transforma falhas em eventos; o transporte nao decide seu formato.
            socket.destroy();
          });
        }
        newlineIndex = buffer.indexOf('\n');
      }
      // Impede que um cliente retenha memória indefinidamente sem fechar uma linha JSON.
      if (Buffer.byteLength(buffer, 'utf8') > MAX_RUNTIME_LINE_BYTES) {
        socket.destroy();
      }
    });
  }
}
