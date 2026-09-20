import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  type BrowserContextAdapter,
  browserConfigFromEnvironment,
  type BrowserPageAdapter,
  BrowserSession,
  parseRequest,
} from '../../src/capabilities/tools/web/browser.js';

test('web.browser aceita operações estruturadas', () => {
  assert.deepEqual(parseRequest('{"operation":"navigate","url":"https://example.test"}'), {
    operation: 'navigate',
    url: 'https://example.test',
  });
  assert.deepEqual(parseRequest('{"operation":"type","selector":"#q","text":"atlas"}'), {
    operation: 'type',
    selector: '#q',
    text: 'atlas',
  });
  for (const operation of ['snapshot', 'scroll', 'screenshot', 'tabs', 'close']) {
    assert.equal(parseRequest(JSON.stringify({ operation })).operation, operation);
  }
});

test('web.browser rejeita operações e URLs inválidos', () => {
  assert.throws(() => parseRequest('{"operation":"navigate","url":"file:///tmp/x"}'), /HTTP/);
  assert.throws(() => parseRequest('{"operation":"click"}'), /selector/);
  assert.throws(() => parseRequest('{"operation":"unknown"}'), /supported/);
});

test('configuração local do browser não depende de regra do Agent', () => {
  const config = browserConfigFromEnvironment({
    ATLAS_WEB_CONFIG_JSON: JSON.stringify({
      browser: { executablePath: '/opt/camoufox', headless: true },
    }),
  });
  assert.equal(config.executablePath, '/opt/camoufox');
  assert.equal(config.headless, true);
  assert.ok(config.socketPath);
});

test('BrowserSession começa fechado e fecha sem inicializar Playwright', async () => {
  const session = new BrowserSession({});
  assert.equal(session.isOpen, false);
  await session.close();
  assert.equal(session.isOpen, false);
});

test('BrowserSession reutiliza o mesmo contexto e fecha o browser residente', async () => {
  let launches = 0;
  let closes = 0;
  let currentUrl = 'about:blank';
  const page: BrowserPageAdapter = {
    goto: async (url) => {
      currentUrl = url;
    },
    title: async () => 'Atlas',
    url: () => currentUrl,
    locator: () => ({
      click: async () => undefined,
      fill: async () => undefined,
      innerText: async () => 'conteúdo',
    }),
    evaluate: async () => undefined,
    screenshot: async () => Buffer.from('png'),
  };
  const context: BrowserContextAdapter = {
    pages: () => [page],
    newPage: async () => page,
    close: async () => {
      closes += 1;
    },
  };
  const session = new BrowserSession({}, async () => {
    launches += 1;
    return context;
  });

  await session.execute({ operation: 'navigate', url: 'https://example.test' });
  await session.execute({ operation: 'snapshot' });
  assert.equal(launches, 1);
  assert.equal(session.isOpen, true);
  await session.close();
  assert.equal(closes, 1);
  assert.equal(session.isOpen, false);
});
