// Sanad chat client.
//
// Two pieces of state that must not be confused:
//
//   transcript — everything the user has seen. Only ever grows.
//   history    — what the model is given next turn. Shrinks as the server
//                compacts it, and is replaced wholesale from the `memory`
//                event. Never edited here.
//
// The turn is committed to `history` only when `memory` arrives. If the stream
// dies mid-answer the user keeps their partial text, but history is untouched,
// so the next question replays correctly instead of silently losing a turn.
(() => {
  "use strict";

  const app = document.querySelector(".chat-app");
  if (!app) return;

  const region = (name) => app.querySelector(`[data-region="${name}"]`);
  const transcriptEl = region("transcript");
  const composer = region("composer");
  const input = composer.querySelector("input");
  const sendButton = composer.querySelector('[data-bind="send"]');
  const drawer = region("drawer");
  const drawerBody = region("drawer-body");
  const backdrop = region("backdrop");
  const savedPanel = region("saved");
  const savedBody = region("saved-body");
  const toastEl = region("toast");
  const bookmarkCount = app.querySelector('[data-bind="bookmark-count"]');

  const BOOKMARK_KEY = "sanad.bookmarks.v1";
  const MAX_BOOKMARKS = 200;

  const state = {
    token: null,
    history: null,
    transcript: [],
    pending: false,
    bookmarks: loadBookmarks(),
  };

  // ---------------------------------------------------------------- helpers

  // Everything is built with createElement/textContent. Model output and
  // hadith text are never trusted as markup.
  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function loadBookmarks() {
    try {
      const raw = JSON.parse(localStorage.getItem(BOOKMARK_KEY) || "null");
      if (!raw || raw.version !== 1 || !Array.isArray(raw.items)) return [];
      return raw.items;
    } catch {
      return [];
    }
  }

  function saveBookmarks() {
    try {
      localStorage.setItem(
        BOOKMARK_KEY,
        JSON.stringify({ version: 1, items: state.bookmarks.slice(0, MAX_BOOKMARKS) }),
      );
    } catch {
      toast("Could not save — your browser storage is full.");
    }
    bookmarkCount.textContent = String(state.bookmarks.length);
  }

  function toast(message) {
    toastEl.textContent = message;
    toastEl.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => {
      toastEl.hidden = true;
    }, 2200);
  }

  function setPending(pending) {
    state.pending = pending;
    input.disabled = pending;
    sendButton.disabled = pending;
    sendButton.textContent = pending ? "…" : "Ask";
  }

  async function ensureSession() {
    if (state.token) return state.token;
    const res = await fetch("/api/chat/session", { method: "POST" });
    if (!res.ok) throw new Error("could not start a chat session");
    const body = await res.json();
    state.token = body.token;
    return state.token;
  }

  // ---------------------------------------------------------------- rendering

  function gradeLabel(hadith) {
    const grade = (hadith.english_grade || "").trim();
    return grade || null;
  }

  function citationCard(hadith) {
    const card = el("article", "chat-card");
    card.dataset.hadithId = String(hadith.hadith_id);

    const meta = el("div", "chat-card-meta");
    meta.append(el("span", "collection", hadith.collection));
    meta.append(
      el("span", null, `Book ${hadith.book_number} · Hadith ${hadith.hadith_number}`),
    );

    const grade = gradeLabel(hadith);
    if (grade) meta.append(el("span", "chat-grade", grade));
    card.append(meta);

    const arabic = el("p", "arabic", hadith.arabic_text);
    arabic.lang = "ar";
    arabic.dir = "rtl";
    card.append(arabic);

    if (hadith.english_text) card.append(el("p", "translation", hadith.english_text));

    const foot = el("div", "chat-card-foot");
    if (hadith.narrator) {
      foot.append(el("span", null, `Narrated by ${hadith.narrator.name}`));
    }
    const open = el("button", "chat-link", "View & related →");
    open.type = "button";
    open.dataset.action = "open-hadith";
    foot.append(open);

    const save = el("button", "chat-link", isSaved(hadith.hadith_id) ? "❖ Saved" : "♢ Save");
    save.type = "button";
    save.dataset.action = "toggle-bookmark";
    foot.append(save);

    card.append(foot);
    return card;
  }

  function renderTurn(turn) {
    const wrap = el("div", "chat-turn");

    const question = el("div", "chat-question");
    question.append(el("p", null, turn.question));
    wrap.append(question);

    const answer = el("div", "chat-answer");
    if (turn.title) answer.append(el("h2", null, turn.title));

    const body = el("div", "chat-answer-body");
    for (const paragraph of (turn.text || "").split("\n")) {
      const trimmed = paragraph.trim();
      if (trimmed) body.append(el("p", null, trimmed));
    }
    answer.append(body);

    if (turn.refused) {
      answer.classList.add("is-refusal");
    }

    if (turn.citations && turn.citations.length) {
      const label = el(
        "p",
        "chat-cite-label",
        `${turn.citations.length} narration${turn.citations.length === 1 ? "" : "s"} found`,
      );
      answer.append(label);
      const list = el("div", "chat-cards");
      for (const hadith of turn.citations) list.append(citationCard(hadith));
      answer.append(list);
    }

    wrap.append(answer);
    return wrap;
  }

  function repaint() {
    transcriptEl.replaceChildren();
    for (const turn of state.transcript) transcriptEl.append(renderTurn(turn));
    app.dataset.chatState = state.transcript.length ? "active" : "empty";
    transcriptEl.scrollTop = transcriptEl.scrollHeight;
    app.scrollTop = app.scrollHeight;
  }

  // ---------------------------------------------------------------- streaming

  // The response is read as a byte stream and split on SSE frame boundaries.
  // Frames do not arrive aligned to chunks, so a partial frame is carried over.
  async function* readEvents(response) {
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      let split;
      while ((split = buffer.indexOf("\n\n")) !== -1) {
        const frame = buffer.slice(0, split);
        buffer = buffer.slice(split + 2);

        let name = "message";
        const data = [];
        for (const line of frame.split("\n")) {
          if (line.startsWith("event:")) name = line.slice(6).trim();
          else if (line.startsWith("data:")) data.push(line.slice(5).trim());
        }
        if (!data.length) continue;
        try {
          yield [name, JSON.parse(data.join("\n"))];
        } catch {
          // A frame we cannot parse is skipped rather than aborting the turn.
        }
      }
    }
  }

  async function ask(message) {
    if (state.pending) return;
    const question = message.trim();
    if (!question) return;

    setPending(true);
    input.value = "";

    // Shown immediately; committed to history only when `memory` arrives.
    const turn = { question, title: "", text: "", citations: [], refused: false };
    state.transcript.push(turn);
    repaint();

    try {
      const token = await ensureSession();
      const response = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json", "x-sanad-session": token },
        body: JSON.stringify({ message: question, history: state.history || undefined }),
      });

      if (!response.ok || !response.body) {
        throw new Error("the assistant is unavailable right now");
      }

      let committed = false;

      for await (const [name, payload] of readEvents(response)) {
        if (name === "title") {
          turn.title = payload.title || "";
        } else if (name === "citations") {
          turn.citations = payload.citations || [];
        } else if (name === "delta") {
          turn.text += payload.text || "";
        } else if (name === "refusal") {
          turn.refused = true;
          turn.title = "";
          turn.text = payload.message || "";
          turn.citations = [];
        } else if (name === "memory") {
          // The server owns history. Replace wholesale, never merge.
          state.history = payload.history;
          committed = true;
        } else if (name === "error") {
          turn.refused = true;
          turn.title = "";
          turn.text = payload.message || "Something went wrong.";
          turn.citations = [];
          if (payload.code === "session_expired") {
            state.token = null;
            turn.text = "This chat has expired. Ask again to start a new one.";
            state.history = null;
          }
        }
        repaint();
      }

      if (!committed) {
        // The stream ended without the authoritative history. Leaving our copy
        // untouched keeps the model's context consistent with what it last saw.
        toast("The connection dropped before that turn was saved.");
      }
    } catch (error) {
      turn.refused = true;
      turn.text = String(error.message || error);
      repaint();
    } finally {
      setPending(false);
      input.focus();
    }
  }

  // ---------------------------------------------------------------- drawer

  function isSaved(id) {
    return state.bookmarks.some((item) => item.hadith_id === id);
  }

  function findHadith(id) {
    for (const turn of state.transcript) {
      for (const hadith of turn.citations || []) {
        if (hadith.hadith_id === id) return hadith;
      }
    }
    return null;
  }

  function toggleBookmark(id) {
    const hadith = findHadith(id);
    if (!hadith) return;

    if (isSaved(id)) {
      state.bookmarks = state.bookmarks.filter((item) => item.hadith_id !== id);
      toast("Removed from your collection");
    } else {
      // Identifiers only. A localStorage copy of canonical text would go stale
      // against the database with no way to notice.
      state.bookmarks.push({
        hadith_id: id,
        collection: hadith.collection,
        book_number: hadith.book_number,
        hadith_number: hadith.hadith_number,
        saved_at: new Date().toISOString(),
      });
      toast("Saved to your collection");
    }
    saveBookmarks();
    repaint();
  }

  async function openDrawer(id) {
    const hadith = findHadith(id);
    if (!hadith) return;

    drawerBody.replaceChildren();
    drawerBody.append(el("p", "chat-muted", "Loading related narrations…"));
    drawer.hidden = false;
    backdrop.hidden = false;

    const head = el("div");
    head.append(el("p", "chat-source", `${hadith.collection} ${hadith.book_number}:${hadith.hadith_number}`));
    const grade = gradeLabel(hadith);
    if (grade) head.append(el("p", "chat-grade", grade));

    const arabic = el("p", "arabic", hadith.arabic_text);
    arabic.lang = "ar";
    arabic.dir = "rtl";
    head.append(arabic);
    if (hadith.english_text) head.append(el("p", "translation", hadith.english_text));
    if (hadith.narrator) head.append(el("p", "chat-muted", `Narrated by ${hadith.narrator.name}`));

    drawerBody.replaceChildren(head);

    try {
      const res = await fetch(`/api/hadiths/${id}/related?limit=3`);
      if (!res.ok) throw new Error("unavailable");
      const body = await res.json();
      const related = body.related || [];
      if (related.length) {
        drawerBody.append(el("h3", null, "Related narrations"));
        const list = el("div", "chat-cards");
        for (const item of related) list.append(citationCard(item));
        drawerBody.append(list);
      }
    } catch {
      drawerBody.append(el("p", "chat-muted", "Related narrations are unavailable right now."));
    }
  }

  function closeOverlays() {
    drawer.hidden = true;
    savedPanel.hidden = true;
    backdrop.hidden = true;
  }

  function openSaved() {
    savedBody.replaceChildren();
    if (!state.bookmarks.length) {
      savedBody.append(el("p", "chat-muted", "No saved narrations yet."));
    } else {
      for (const item of state.bookmarks) {
        const row = el("div", "chat-saved-row");
        row.append(
          el("span", null, `${item.collection} ${item.book_number}:${item.hadith_number}`),
        );
        savedBody.append(row);
      }
    }
    savedPanel.hidden = false;
    backdrop.hidden = false;
  }

  // ---------------------------------------------------------------- events

  composer.addEventListener("submit", (event) => {
    event.preventDefault();
    ask(input.value);
  });

  app.addEventListener("click", (event) => {
    const prompt = event.target.closest("[data-prompt]");
    if (prompt) {
      ask(prompt.dataset.prompt);
      return;
    }

    const action = event.target.closest("[data-action]");
    if (!action) return;

    const card = action.closest("[data-hadith-id]");
    const id = card ? Number(card.dataset.hadithId) : null;

    switch (action.dataset.action) {
      case "open-hadith":
        openDrawer(id);
        break;
      case "toggle-bookmark":
        toggleBookmark(id);
        break;
      case "open-saved":
        openSaved();
        break;
      case "close-drawer":
      case "close-saved":
        closeOverlays();
        break;
    }
  });

  backdrop.addEventListener("click", closeOverlays);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") closeOverlays();
  });

  bookmarkCount.textContent = String(state.bookmarks.length);
})();
