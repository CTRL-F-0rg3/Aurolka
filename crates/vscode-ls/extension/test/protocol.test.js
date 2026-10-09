"use strict";

const assert = require("node:assert/strict");
const { test } = require("node:test");

// Moduł `vscode` istnieje tylko w Extension Host, więc podmieniamy go
// atrapą — dzięki temu testy parsera ramkującego działają w czystym Node.
const atrapa = {
  workspace: {
    getConfiguration: () => ({ get: () => "" }),
    workspaceFolders: undefined,
  },
  window: {
    createOutputChannel: () => ({ appendLine() {}, show() {} }),
    createStatusBarItem: () => ({ show() {} }),
    showErrorMessage() {},
    showInformationMessage() {},
    activeTextEditor: undefined,
  },
  StatusBarAlignment: { Left: 1 },
  commands: { registerCommand() {} },
  languages: { createDiagnosticCollection: () => ({ delete() {}, set() {} }) },
  DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
  Position: class {
    constructor(line, character) {
      this.line = line;
      this.character = character;
    }
  },
  Range: class {
    constructor(start, end) {
      this.start = start;
      this.end = end;
    }
  },
  SymbolInformation: class {
    constructor(name, kind, range, uri) {
      Object.assign(this, { name, kind, range, uri });
    }
  },
  Uri: { parse: (tekst) => ({ toString: () => tekst }) },
};

const Module = require("node:module");
const oryginalne = Module._load;
Module._load = function (szukany, ...reszta) {
  if (szukany === "vscode") {
    return atrapa;
  }
  return oryginalne.call(this, szukany, ...reszta);
};

const { BridgeClient, JĘZYKI } = require("../client.js");

/** Klient bez procesu — testujemy wyłącznie ramkowanie i parsowanie. */
function klient() {
  const wyjscie = [];
  const c = new BridgeClient(
    { statusBar: { show() {} } },
    { appendLine: (linia) => wyjscie.push(linia) },
  );
  c.logi = wyjscie;
  return c;
}

/** Buduje ramkę `Content-Length` dokładnie tak, jak robi to serwer. */
function ramka(obiekty) {
  const lista = Array.isArray(obiekty) ? obiekty : [obiekty];
  const czesci = [];
  for (const obiekt of lista) {
    const tresc = Buffer.from(JSON.stringify(obiekt), "utf8");
    czesci.push(Buffer.from(`Content-Length: ${tresc.length}\r\n\r\n`, "ascii"), tresc);
  }
  return Buffer.concat(czesci);
}

/** Pusty chunk — `onData` ma czytać tylko z wewnętrznego bufora. */
const NIC = Buffer.alloc(0);

test("ramka z jedną wiadomością wraca jako cała", () => {
  const c = klient();
  c.buffer = ramka({ jsonrpc: "2.0", method: "test", params: {} });

  c.onData(NIC);

  assert.equal(c.buffer.length, 0, "bufor musi być opróżniony");
  assert.deepEqual(c.logi, ["nieznana metoda: test"]);
});

test("dwie wiadomości w jednym chunku", () => {
  const c = klient();
  c.buffer = ramka([
    { jsonrpc: "2.0", method: "pierwsza", params: {} },
    { jsonrpc: "2.0", method: "druga", params: {} },
  ]);

  c.onData(NIC);

  assert.equal(c.buffer.length, 0);
  assert.deepEqual(c.logi, ["nieznana metoda: pierwsza", "nieznana metoda: druga"]);
});

test("payload dochodzi w kilku chunkach", () => {
  const c = klient();
  const pelne = ramka({ jsonrpc: "2.0", method: "rozcięta", params: {} });

  // Nagłówek na pewno się mieści, payload zostawiamy w połowie.
  const koniecNaglowka = pelne.indexOf("\r\n\r\n") + 4;
  c.onData(pelne.subarray(0, koniecNaglowka + 3));
  assert.deepEqual(c.logi, [], "niepełna ramka nie może niczego zgłosić");

  c.onData(pelne.subarray(koniecNaglowka + 3));
  assert.deepEqual(c.logi, ["nieznana metoda: rozcięta"]);
});

test("długość liczona jest w bajtach, nie w znakach", () => {
  const c = klient();
  // `😀` to 4 bajty UTF-8, więc `Content-Length` musi je uwzględnić.
  c.buffer = ramka({ jsonrpc: "2.0", method: "zażółć", params: { x: "😀" } });

  c.onData(NIC);

  assert.deepEqual(c.logi, ["nieznana metoda: zażółć"]);
});

test("ramka bez Content-Length jest pomijana", () => {
  const c = klient();
  const tresc = Buffer.from(JSON.stringify({ jsonrpc: "2.0", method: "x" }), "utf8");
  c.buffer = Buffer.concat([
    Buffer.from("X-Nic: 1\r\n\r\n", "ascii"),
    tresc,
    ramka({ jsonrpc: "2.0", method: "dobra", params: {} }),
  ]);

  c.onData(NIC);

  // Pierwsza wiadomość ginie, kolejna jest już poprawna.
  assert.deepEqual(c.logi, ["nieznana metoda: dobra"]);
});

test("symbole wracają jako SymbolInformation", async () => {
  const c = klient();
  const zapytania = [];
  c.sendRequest = (method, params) => {
    zapytania.push([method, params]);
    return Promise.resolve([
      {
        name: "main",
        kind: 12,
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 12 } },
      },
    ]);
  };

  const symbole = await c.documentSymbols("file:///a.rs");

  assert.deepEqual(zapytania[0][0], "textDocument/documentSymbol");
  assert.deepEqual(zapytania[0][1], { textDocument: { uri: "file:///a.rs" } });
  assert.equal(symbole.length, 1);
  assert.equal(symbole[0].name, "main");
  assert.equal(symbole[0].kind, 12);
  assert.equal(symbole[0].range.end.character, 12);
});

test("pusta lista symboli nie jest błędem", async () => {
  const c = klient();
  c.sendRequest = () => Promise.resolve([]);

  assert.deepEqual(await c.documentSymbols("file:///a.rs"), []);
  assert.deepEqual(c.logi, []);
});

test("błąd serwera nie wywraca panelu Outline", async () => {
  const c = klient();
  c.sendRequest = () => Promise.reject(new Error("serwer nie odpowiada"));

  assert.deepEqual(await c.documentSymbols("file:///a.rs"), []);
  assert.equal(c.logi.length, 1);
  assert.match(c.logi[0], /nie odpowiada/);
});

test("serwer wywołuje komendę klienta", async () => {
  const wykonane = [];
  atrapa.commands.executeCommand = (...args) => {
    wykonane.push(args);
  };

  const c = klient();
  c.handleMessage({
    jsonrpc: "2.0",
    method: "workspace/executeCommand",
    params: {
      command: "workbench.action.terminal.sendSequence",
      arguments: ["cargo test"],
    },
  });

  assert.deepEqual(wykonane, [
    ["workbench.action.terminal.sendSequence", "cargo test"],
  ]);
});

test("lista języków pokrywa te, które deklaruje manifest", () => {
  assert.deepEqual([...JĘZYKI].sort(), ["javascript", "rust", "typescript"]);
});