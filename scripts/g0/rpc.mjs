import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

/** JSON-lines transport; refuses to silently continue after a process failure. */
export class AppServer {
  #nextId = 1;
  #pending = new Map();
  notifications = [];
  stderr = '';

  constructor(binary, environment) {
    this.child = spawn(binary, ['app-server', '--stdio'], {
      env: environment,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', text => { this.stderr = (this.stderr + text).slice(-20000); });
    createInterface({ input: this.child.stdout }).on('line', line => {
      let message;
      try { message = JSON.parse(line); } catch { return; }
      const pending = this.#pending.get(message.id);
      if (pending) {
        clearTimeout(pending.timer);
        this.#pending.delete(message.id);
        if (message.error) pending.reject(new Error(JSON.stringify(message.error)));
        else pending.resolve(message.result);
      } else {
        this.notifications.push(message);
      }
    });
    this.child.on('error', error => this.#fail(error));
    this.child.on('exit', (code, signal) => this.#fail(new Error(`app-server exited: ${code ?? signal}; ${this.stderr}`)));
  }

  #fail(error) {
    for (const entry of this.#pending.values()) { clearTimeout(entry.timer); entry.reject(error); }
    this.#pending.clear();
  }

  notify(method, params = {}) {
    this.child.stdin.write(JSON.stringify({ method, params }) + '\n');
  }

  call(method, params = {}, timeout = 20000) {
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`Timeout: ${method}; ${this.stderr}`));
      }, timeout);
      this.#pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(JSON.stringify({ id, method, params }) + '\n');
    });
  }

  async initialize() {
    const result = await this.call('initialize', {
      clientInfo: { name: 'gptswitch_g0', title: 'Switchelp isolated probe', version: '0.1.0' },
      capabilities: { experimentalApi: true },
    });
    this.notify('initialized');
    return result;
  }

  stop() { this.child.stdin.end(); this.child.kill('SIGTERM'); }
}
