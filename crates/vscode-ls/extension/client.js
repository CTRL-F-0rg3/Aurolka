/**
 * Klient rozszerzenia Aurola.
 *
 * Uruchamia `vscode-ls` (serwer LSP napisany w Rustcie), przekazuje mu
 * wiadomości ramkowane przez `Content-Length` i obsługuje powiadomienia
 * w stronę edytora (`window/showInformationMessage`, `vscode/open`,
 * `vscode/insertText`, `vscode/setStatus`, `window/logMessage`).
 *
 * Celowo bez zależności — wystarczy czysty Node z `child_process`.
 */

"use strict";

const vscode = require("vscode");
const path = require("node:path");
const fs = require("node:fs");
const { spawn } = require("node:child_process");

/** Języki, które obsługuje serwer. */
const JĘZYKI = ["rust", "typescript", "javascript"];

/** Klient LSP z uruchomionym procesem. */
class BridgeClient {
  constructor(context, output) {
    this.context = context;
    this.output = output;
    this.process = null;
    this.buffer = Buffer.alloc(0);
    this.requestId = 1;
    this.pending = new Map();
    this.restartCount = 0;
    this.diagnostics = vscode.languages.createDiagnosticCollection("aurola");
  }

  /** Czy ten dokument nas dotyczy. */
  shouldTrack(document) {
    return JĘZYKI.includes(document.languageId);
  }

  /** Ścieżka do serwera: ustawienie → PATH → katalog repozytorium. */
  resolveServerPath() {
    const configured = vscode.workspace
      .getConfiguration("aurola")
      .get("serverPath", "")
      .trim();

    if (configured) {
      return configured;
    }

    // Ostatnia deska ratunku: serwer zbudowany w tym samym repozytorium.
    const root = vscode.workspace.workspaceFolders?.[0]?.uri?.fsPath;
    if (root) {
      const local = path.join(root, "target", "debug", "vscode-ls");
      if (fs.existsSync(local)) {
        return local;
      }
    }

    return "vscode-ls";
  }

  /** Uruchamia proces serwera i wysyła `initialize`. */
  start() {
    const binary = this.resolveServerPath();
    this.output.appendLine(`uruchamiam: ${binary}`);

    this.process = spawn(binary, [], { stdio: ["pipe", "pipe", "pipe"] });
    this.process.stdout.on("data", (chunk) => this.onData(chunk));
    this.process.stderr.on("data", (chunk) => {
      this.output.appendLine(`serwer stderr: ${chunk.toString().trimEnd()}`);
    });
    this.process.on("error", (error) => {
      this.output.appendLine(`nie udało się uruchomić serwera: ${error.message}`);
      vscode.window.showErrorMessage(
        `Aurola: nie udało się uruchomić serwera (${error.message}). Ustaw "aurola.serverPath".`,
      );
    });
    this.process.on("exit", (code) => {
      this.output.appendLine(`serwer zakończony (kod ${code})`);
    });

    this.sendRequest("initialize", {
      processId: process.pid,
      rootUri: vscode.workspace.workspaceFolders?.[0]?.uri.toString() ?? null,
      workspaceFolders: (vscode.workspace.workspaceFolders ?? []).map((f) => ({
        uri: f.uri.toString(),
        name: f.name,
      })),
      capabilities: {
        general: { positionEncodings: ["utf-16"] },
        textDocument: {
          synchronization: { dynamicRegistration: false },
          publishDiagnostics: { versionSupport: true },
        },
      },
    });

    this.sendNotification("initialized", {});
  }

  /** Zatrzymuje proces. */
  stop() {
    if (!this.process) {
      return;
    }
    try {
      this.sendNotification("shutdown", null);
      this.sendNotification("exit", null);
    } catch {
      // proces może już nie żyć — nie zgłaszamy tego drugi raz
    }
    this.process.kill();
    this.process = null;
  }

  /** Restart z limitem prób, żeby nie zapętlić się przy stałym błędzie. */
  restart() {
    if (this.restartCount >= 3) {
      vscode.window.showErrorMessage(
        "Aurola: serwer restartuje się w pętli — sprawdź kanał wyjściowy.",
      );
      return;
    }
    this.restartCount += 1;
    this.stop();
    this.start();
  }

  /** Wysyła żądanie i czeka na odpowiedź. */
  sendRequest(method, params) {
    const id = this.requestId++;
    this.write({ jsonrpc: "2.0", id, method, params });
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
  }

  /** Wysyła powiadomienie (bez odpowiedzi). */
  sendNotification(method, params) {
    this.write({ jsonrpc: "2.0", method, params });
  }

  /** Ramkuje wiadomość nagłówkiem `Content-Length`. */
  write(message) {
    if (!this.process || !this.process.stdin.writable) {
      return;
    }
    const body = Buffer.from(JSON.stringify(message), "utf8");
    this.process.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
    this.process.stdin.write(body);
  }

  /** Składa strumień w całe wiadomości i obsługuje je. */
  onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);

    for (;;) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) {
        return;
      }

      const header = this.buffer.subarray(0, headerEnd).toString("ascii");
      const match = /Content-Length:\s*(\d+)/i.exec(header);
      if (!match) {
        // ramka bez długości — nie da się jej zinterpretować
        this.buffer = this.buffer.subarray(headerEnd + 4);
        continue;
      }

      const length = Number.parseInt(match[1], 10);
      const start = headerEnd + 4;
      if (this.buffer.length < start + length) {
        return; // payload jeszcze nie w całości
      }

      const body = this.buffer.subarray(start, start + length).toString("utf8");
      this.buffer = this.buffer.subarray(start + length);

      try {
        this.handleMessage(JSON.parse(body));
      } catch (error) {
        this.output.appendLine(`zła wiadomość: ${error.message}`);
      }
    }
  }

  /** Wykonuje pojedynczą wiadomość z serwera. */
  handleMessage(message) {
    if (message.id !== undefined && this.pending.has(message.id)) {
      const { resolve, reject } = this.pending.get(message.id);
      this.pending.delete(message.id);
      if (message.error) {
        reject(new Error(message.error.message));
      } else {
        resolve(message.result);
      }
      return;
    }

    switch (message.method) {
      case "textDocument/publishDiagnostics":
        this.applyDiagnostics(message.params);
        break;
      case "window/showInformationMessage":
        vscode.window.showInformationMessage(message.params?.message ?? "");
        break;
      case "window/showWarningMessage":
        vscode.window.showWarningMessage(message.params?.message ?? "");
        break;
      case "window/showErrorMessage":
        vscode.window.showErrorMessage(message.params?.message ?? "");
        break;
      case "window/logMessage":
        this.output.appendLine(message.params?.message ?? "");
        break;
      case "vscode/open":
        this.openDocument(message.params?.uri);
        break;
      case "vscode/insertText":
        this.insertText(message.params);
        break;
      case "vscode/setStatus":
        this.setStatus(message.params?.text ?? "");
        break;
      case "workspace/executeCommand":
        this.runCommand(message.params);
        break;
      default:
        this.output.appendLine(`nieznana metoda: ${message.method}`);
    }
  }

  /** Wykonuje komendę VS Code na życzenie serwera. */
  async runCommand(params) {
    const command = params?.command;
    if (!command) {
      return;
    }
    try {
      await vscode.commands.executeCommand(command, ...(params.arguments ?? []));
    } catch (error) {
      this.output.appendLine(`komenda ${command} nie powiodła się: ${error.message}`);
    }
  }

  /**
   * Symbole dokumentu w formacie VS Code.
   *
   * Serwer zwraca płaską listę (`SymbolInformation`), a VS Code oczekuje
   * obiektów z `kind` jako numerem i zakresem w postaci pozycji.
   */
  async documentSymbols(uri) {
    try {
      const symbole = await this.sendRequest("textDocument/documentSymbol", {
        textDocument: { uri: uri.toString() },
      });
      return (symbole ?? []).map((symbol) => new vscode.SymbolInformation(
        symbol.name,
        symbol.kind,
        new vscode.Range(
          new vscode.Position(symbol.range.start.line, symbol.range.start.character),
          new vscode.Position(symbol.range.end.line, symbol.range.end.character),
        ),
        uri,
      ));
    } catch (error) {
      this.output.appendLine(`symbole dla ${uri.toString()}: ${error.message}`);
      return [];
    }
  }

  /** Wstawia diagnostyki do kolekcji VS Code. */
  applyDiagnostics(params) {
    if (!params?.uri) {
      return;
    }
    const uri = vscode.Uri.parse(params.uri);
    const items = (params.diagnostics ?? []).map((d) => {
      const start = new vscode.Position(d.range.start.line, d.range.start.character);
      const end = new vscode.Position(d.range.end.line, d.range.end.character);

      let severity = vscode.DiagnosticSeverity.Information;
      // Serwer wysyła liczby 1–4 zgodnie ze specyfikacją LSP.
      if (d.severity === 1) severity = vscode.DiagnosticSeverity.Error;
      if (d.severity === 2) severity = vscode.DiagnosticSeverity.Warning;
      if (d.severity === 3) severity = vscode.DiagnosticSeverity.Information;
      if (d.severity === 4) severity = vscode.DiagnosticSeverity.Hint;

      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(start, end),
        d.message,
        severity,
      );
      if (d.source) diagnostic.source = d.source;
      if (d.code) diagnostic.code = d.code;
      return diagnostic;
    });

    this.diagnostics.set(uri, items);
  }

  /** Otwiera plik w edytorze. */
  async openDocument(uri) {
    if (!uri) {
      return;
    }
    const document = await vscode.workspace.openTextDocument(vscode.Uri.parse(uri));
    await vscode.window.showTextDocument(document);
  }

  /** Wstawia tekst w pozycji podanej przez serwer. */
  async insertText(params) {
    if (!params?.uri) {
      return;
    }
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.uri.toString() !== params.uri) {
      return;
    }

    const position = new vscode.Position(params.position.line, params.position.character);
    const success = await editor.edit((builder) => builder.insert(position, params.text ?? ""));
    if (!success) {
      this.output.appendLine("nie udało się wstawić tekstu");
    }
  }

  /** Ustawia tekst w pasku stanu. */
  setStatus(text) {
    this.context.statusBar.text = text ? `$(check) ${text}` : "";
    this.context.statusBar.show();
  }
}let client;

/** Punkt wejścia rozszerzenia. */
function activate(context) {
  const output = vscode.window.createOutputChannel("Aurola");
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);

  context.subscriptions.push(output, statusBar);

  client = new BridgeClient({ statusBar }, output);
  client.restart();

  context.subscriptions.push(
    vscode.commands.registerCommand("vscodeBridge.status", () => {
      vscode.window.showInformationMessage(
        `Aurola: serwer działa, śledzonych plików: ${client.diagnostics.size}.`,
      );
    }),

    vscode.commands.registerCommand("vscodeBridge.reanalyzeAll", () => {
      let liczba = 0;
      for (const document of vscode.workspace.textDocuments) {
        if (!client.shouldTrack(document)) {
          continue;
        }
        client.sendNotification("textDocument/didSave", {
          textDocument: { uri: document.uri.toString() },
        });
        liczba += 1;
      }
      vscode.window.showInformationMessage(`Aurola: ponowna analiza ${liczba} plików.`);
    }),

    vscode.commands.registerCommand("vscodeBridge.restart", () => {
      client.restart();
      vscode.window.showInformationMessage("Aurola: serwer uruchomiony ponownie.");
    }),

    vscode.commands.registerCommand("vscodeBridge.showOutput", () => output.show()),
  );

  // Synchronizacja tekstu z edytora do serwera.
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => {
      if (!client.shouldTrack(document)) {
        return;
      }
      client.sendNotification("textDocument/didOpen", {
        textDocument: {
          uri: document.uri.toString(),
          languageId: document.languageId,
          version: document.version,
          text: document.getText(),
        },
      });
    }),

    vscode.workspace.onDidChangeTextDocument((event) => {
      if (!client.shouldTrack(event.document)) {
        return;
      }
      client.sendNotification("textDocument/didChange", {
        textDocument: {
          uri: event.document.uri.toString(),
          version: event.document.version,
        },
        // Serwer obsługuje wariant pełnego tekstu.
        contentChanges: [{ text: event.document.getText() }],
      });
    }),

    vscode.workspace.onDidCloseTextDocument((document) => {
      if (!client.shouldTrack(document)) {
        return;
      }
      client.diagnostics.delete(document.uri);
      client.sendNotification("textDocument/didClose", {
        textDocument: { uri: document.uri.toString() },
      });
    }),
  );

  // Panel „Outline”: symbole dokumentu dla języków, które obsługuje serwer.
  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      JĘZYKI.map((language) => ({ language })),
      {
        provideDocumentSymbols: (document) =>
          client.shouldTrack(document) ? client.documentSymbols(document.uri) : [],
      },
    ),
  );
}

/** Wywoływane przy wyłączeniu rozszerzenia. */
function deactivate() {
  client?.stop();
}

module.exports = { activate, deactivate, BridgeClient, JĘZYKI };