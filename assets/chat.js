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
  const input = composer.querySelector("textarea");
  const sendButton = composer.querySelector('[data-bind="send"]');
  // The glyph lives in its own span so the pending state can swap it without
  // clearing the button's accessible name.
  const sendIcon = composer.querySelector('[data-bind="send-icon"]');
  const drawer = region("drawer");
  const drawerBody = region("drawer-body");
  const backdrop = region("backdrop");
  const toastEl = region("toast");
  const replyEl = region("reply");
  const replyRef = composer.querySelector('[data-bind="reply-ref"]');

  const state = {
    token: null,
    history: null,
    transcript: [],
    pending: false,
    // Every narration the reader has been shown a reference to, by id.
    //
    // A related narration reached through the drawer is not in the transcript,
    // so looking only there left its reference dead on click.
    seen: new Map(),
    // The narration staged for the next question, or null.
    replyTo: null,
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

  // About half the source records carry HTML in their text. DOMParser decodes
  // entities and drops tags without executing anything, and block elements
  // become line breaks so sentences the markup separated stay separated.
  //
  // Newlines already in the source are hard wrapping, not structure: across the
  // corpus they land mid-sentence far more often than at a sentence end. They
  // are flattened out of the text nodes first, so the only newlines left to
  // split on are the ones the block tags introduce. Mirrors `to_plain_text` in
  // src/text.rs; the two must agree on what counts as a paragraph.
  function plainText(raw) {
    if (!raw) return "";
    if (!raw.includes("<") && !raw.includes("&")) return raw.replace(/\s+/g, " ").trim();

    const doc = new DOMParser().parseFromString(raw, "text/html");
    const text = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
    for (let node = text.nextNode(); node; node = text.nextNode()) {
      node.textContent = node.textContent.replace(/\s+/g, " ");
    }
    for (const node of doc.body.querySelectorAll("p, br, div, li, tr, h1, h2, h3")) {
      node.before("\n");
    }
    return doc.body.textContent
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .join("\n");
  }

  function paragraphsInto(parent, text) {
    for (const paragraph of plainText(text).split("\n")) {
      if (paragraph) parent.append(el("p", null, paragraph));
    }
  }

  function toast(message) {
    toastEl.textContent = message;
    toastEl.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => {
      toastEl.hidden = true;
    }, 2200);
  }

  // Height is set from content because a textarea does not grow on its own.
  // Reset to auto first: scrollHeight only shrinks once the box is smaller
  // than its content.
  const COMPOSER_MAX_PX = 160;

  function resizeComposer() {
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, COMPOSER_MAX_PX)}px`;
  }

  function setPending(pending) {
    state.pending = pending;
    input.disabled = pending;
    sendButton.disabled = pending;
    sendIcon.textContent = pending ? "…" : "↑";
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

  // Appends the narration text itself — Arabic, then translation.
  //
  // Shared by the citation card and the drawer so the two cannot drift on how
  // canonical text is tagged; lang/dir in particular are correctness for a
  // right-to-left script, not decoration.
  function appendNarrationText(target, hadith) {
    const arabic = el("p", "arabic", plainText(hadith.arabic_text));
    arabic.lang = "ar";
    arabic.dir = "rtl";
    target.append(arabic);

    if (hadith.english_text) {
      const translation = el("div", "translation");
      paragraphsInto(translation, hadith.english_text);
      target.append(translation);
    }
  }

  // A narration as a reference that opens it, rather than as a card.
  //
  // The drawer is where a narration is read: it has the room for the full
  // Arabic, the translation, the grading and the related narrations. Repeating
  // all of that inline buried the answer under the sources it was built from,
  // and gave the same narration two different presentations.
  function referenceLink(hadith) {
    remember(hadith);

    const link = el("button", "chat-cite", hadithRef(hadith));
    link.type = "button";
    link.dataset.action = "open-hadith";
    link.dataset.hadithId = String(hadith.hadith_id);
    return link;
  }

  /// Appends `hadiths` as a comma-separated run of reference links.
  function appendReferences(parent, hadiths) {
    hadiths.forEach((hadith, index) => {
      if (index > 0) parent.append(document.createTextNode(", "));
      parent.append(referenceLink(hadith));
    });
  }

  // The canonical citation: collection title plus hadith number, the same form
  // sunnah.com prints as its reference. The book number is a secondary
  // reference and belongs in the drawer rather than in a citation.
  function hadithRef(hadith) {
    return `${hadith.collection_name || hadith.collection} ${hadith.hadith_number}`;
  }

  // Renders the answer, turning the model's [n] markers into links.
  //
  // The model writes only the number; the reference itself is built here from
  // the retrieved record, so it cannot cite a narration that was not retrieved
  // and cannot misspell one that was. A marker with no matching citation — [4]
  // where three were retrieved — is left as plain text rather than linked, so
  // the failure is visible instead of pointing somewhere wrong.
  const CITATION = /\[(\d+)\]/g;

  /// Which markers in `text` resolve to a retrieved narration.
  function resolvedCitations(text, citations) {
    const found = new Set();
    for (const match of (text || "").matchAll(CITATION)) {
      const index = Number(match[1]) - 1;
      if (citations[index]) found.add(index);
    }
    return found;
  }

  // The subset of Markdown the model is told it may use. An answer that walks
  // through the steps of something reads far better as a list than as one block
  // of prose, and a bolded lead-in is what makes the steps findable — but the
  // syntax has to be turned into elements to be worth anything, or the reader
  // just sees the asterisks.
  //
  // Bold is tried before italics at each position, so `**x**` is never read as
  // an empty italic. A bullet needs whitespace after its marker, which is what
  // keeps `**During the prayer:**` from parsing as a list item.
  const EMPHASIS = /\*\*([^*]+)\*\*|\*([^*\n]+)\*/g;
  const BULLET = /^[-*+]\s+(.*)$/;
  const NUMBERED = /^\d+[.)]\s+(.*)$/;

  // Nothing here ever assigns markup. Emphasis and list structure become real
  // elements built with createElement, so model output stays text throughout.
  function appendCited(parent, text, citations) {
    let last = 0;

    for (const match of text.matchAll(CITATION)) {
      const hadith = citations[Number(match[1]) - 1];
      const before = text.slice(last, match.index);
      if (before) parent.append(document.createTextNode(before));

      // `[1][2]` arrives with nothing between the markers, which renders as
      // two references run together. They are a list, so they read as one.
      if (!before && parent.lastChild && parent.lastChild.classList?.contains("chat-cite")) {
        parent.append(document.createTextNode(", "));
      }

      if (hadith) {
        const cite = el("button", "chat-cite", hadithRef(hadith));
        cite.type = "button";
        cite.dataset.action = "open-hadith";
        // The handler reads the id from the nearest element carrying it, and
        // `closest` starts at the element itself, so the marker needs no
        // wrapping card.
        cite.dataset.hadithId = String(hadith.hadith_id);
        parent.append(cite);
      } else {
        parent.append(document.createTextNode(match[0]));
      }
      last = match.index + match[0].length;
    }

    const rest = text.slice(last);
    if (rest) parent.append(document.createTextNode(rest));
  }

  // Emphasis first, then citations within each span: a marker inside a bolded
  // lead-in has to stay a working reference rather than becoming literal text.
  function appendInline(parent, text, citations) {
    let last = 0;

    for (const match of text.matchAll(EMPHASIS)) {
      const before = text.slice(last, match.index);
      if (before) appendCited(parent, before, citations);

      const bold = match[1];
      const span = el(bold ? "strong" : "em");
      appendCited(span, bold || match[2], citations);
      parent.append(span);

      last = match.index + match[0].length;
    }

    const rest = text.slice(last);
    if (rest) appendCited(parent, rest, citations);
  }

  function appendAnswerText(parent, text, citations) {
    // The list currently being filled, so consecutive items land in one list
    // rather than each becoming a list of its own. A blank line, a paragraph,
    // or a switch between bulleted and numbered closes it.
    let list = null;
    let listTag = null;

    for (const line of (text || "").split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) {
        list = null;
        continue;
      }

      const bullet = trimmed.match(BULLET);
      const numbered = bullet ? null : trimmed.match(NUMBERED);
      const item = bullet ? bullet[1] : numbered ? numbered[1] : null;

      if (item === null) {
        list = null;
        const p = el("p");
        appendInline(p, trimmed, citations);
        parent.append(p);
        continue;
      }

      const tag = bullet ? "ul" : "ol";
      if (!list || listTag !== tag) {
        list = el(tag, "chat-answer-list");
        listTag = tag;
        parent.append(list);
      }

      const li = el("li");
      appendInline(li, item, citations);
      list.append(li);
    }
  }

  function stageReply(id) {
    const hadith = findHadith(id);
    if (!hadith) return;

    state.replyTo = {
      hadith_id: id,
      collection: hadith.collection,
      book_number: hadith.book_number,
      hadith_number: hadith.hadith_number,
    };
    renderReply();
    input.focus();
  }

  function clearReply() {
    state.replyTo = null;
    renderReply();
  }

  function renderReply() {
    if (!state.replyTo) {
      replyEl.hidden = true;
      replyRef.textContent = "";
      return;
    }
    replyRef.textContent = hadithRef(state.replyTo);
    replyEl.hidden = false;
  }

  function renderTurn(turn) {
    const wrap = el("div", "chat-turn");

    const question = el("div", "chat-question");
    if (turn.replyTo) {
      // Kept beside the question so the transcript still shows which narration
      // was being asked about after the composer has been cleared.
      question.append(el("p", "chat-question-ref", `↩ ${hadithRef(turn.replyTo)}`));
    }
    question.append(el("p", null, turn.question));
    wrap.append(question);

    const answer = el("div", "chat-answer");

    const body = el("div", "chat-answer-body");
    appendAnswerText(body, turn.text, turn.citations || []);
    answer.append(body);

    if (turn.refused) {
      answer.classList.add("is-refusal");
    }

    // Only when the answer cited nothing itself. Otherwise every narration it
    // used is already named in the prose and one click from being read, and
    // listing them again underneath says the same thing twice.
    const citations = turn.citations || [];
    const citedInline = resolvedCitations(turn.text, citations).size > 0;

    if (citations.length && !citedInline) {
      answer.append(el("p", "chat-cite-label", "Narrations found"));
      const refs = el("p", "chat-references");
      appendReferences(refs, citations);
      answer.append(refs);
    } else {
      // Reachable by reference even when the prose cites only some of them.
      citations.forEach(remember);
    }

    wrap.append(answer);
    return wrap;
  }

  // How close to the bottom still counts as following along.
  const FOLLOW_THRESHOLD_PX = 120;

  function atBottom() {
    const distance =
      transcriptEl.scrollHeight - transcriptEl.scrollTop - transcriptEl.clientHeight;
    return distance < FOLLOW_THRESHOLD_PX;
  }

  // `force` scrolls regardless; used when the reader has just sent a question,
  // where jumping to it is the point.
  function repaint(force) {
    // Read before replacing the children, since that resets the scroll offset.
    // An answer streams dozens of repaints, and scrolling on each of them made
    // it impossible to read back through the transcript while one arrived —
    // every keystroke of the model's yanked the view back down.
    const follow = force || atBottom();

    transcriptEl.replaceChildren();
    for (const turn of state.transcript) transcriptEl.append(renderTurn(turn));
    app.dataset.chatState = state.transcript.length ? "active" : "empty";

    if (follow) transcriptEl.scrollTop = transcriptEl.scrollHeight;
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
    resizeComposer();

    // The staged narration rides along on the turn so the transcript can show
    // it, and the composer is cleared either way. It is not sent to the server
    // yet: how a narration should shape the answer is still to be decided.
    const replyTo = state.replyTo;
    clearReply();

    // Shown immediately; committed to history only when `memory` arrives.
    const turn = { question, text: "", citations: [], refused: false, replyTo };
    state.transcript.push(turn);
    // Forced: the reader just asked, so showing them their own question is
    // what they expect, wherever they had scrolled to.
    repaint(true);

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
        if (name === "citations") {
          turn.citations = payload.citations || [];
        } else if (name === "delta") {
          turn.text += payload.text || "";
        } else if (name === "refusal") {
          turn.refused = true;
          turn.text = payload.message || "";
          turn.citations = [];
        } else if (name === "memory") {
          // The server owns history. Replace wholesale, never merge.
          state.history = payload.history;
          committed = true;
        } else if (name === "error") {
          turn.refused = true;
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

  function remember(hadith) {
    state.seen.set(hadith.hadith_id, hadith);
  }

  function findHadith(id) {
    for (const turn of state.transcript) {
      for (const hadith of turn.citations || []) {
        if (hadith.hadith_id === id) return hadith;
      }
    }
    return state.seen.get(id) || null;
  }

  async function openDrawer(id) {
    const hadith = findHadith(id);
    if (!hadith) return;

    drawerBody.replaceChildren();
    drawerBody.append(el("p", "chat-muted", "Loading related narrations…"));
    drawer.hidden = false;
    backdrop.hidden = false;

    const head = el("div");
    // The canonical citation, not the internal slug and book:number form.
    head.append(el("p", "chat-source", hadithRef(hadith)));
    head.append(
      el("p", "chat-muted", `Book ${hadith.book_number} · Hadith ${hadith.hadith_number}`),
    );
    const grade = gradeLabel(hadith);
    if (grade) head.append(el("p", "chat-grade", grade));

    appendNarrationText(head, hadith);
    if (hadith.narrator) head.append(el("p", "chat-muted", `Narrated by ${hadith.narrator.name}`));

    // Moved here from the citation card: this is where the narration is read,
    // so it is where deciding to ask about it belongs.
    const ask = el("button", "chat-link", "↩ Ask about this");
    ask.type = "button";
    ask.dataset.action = "reply-hadith";
    ask.dataset.hadithId = String(hadith.hadith_id);
    head.append(ask);

    drawerBody.replaceChildren(head);

    try {
      const res = await fetch(`/api/hadiths/${id}/related?limit=3`);
      if (!res.ok) throw new Error("unavailable");
      const body = await res.json();
      const related = body.related || [];
      if (related.length) {
        drawerBody.append(el("h3", null, "Related narrations"));
        const refs = el("p", "chat-references");
        appendReferences(refs, related);
        drawerBody.append(refs);
      }
    } catch {
      drawerBody.append(el("p", "chat-muted", "Related narrations are unavailable right now."));
    }
  }

  function closeOverlays() {
    drawer.hidden = true;
    backdrop.hidden = true;
  }

  // ---------------------------------------------------------------- events

  composer.addEventListener("submit", (event) => {
    event.preventDefault();
    ask(input.value);
  });

  input.addEventListener("input", resizeComposer);

  // Enter sends, Shift+Enter opens a line. A textarea does neither by default:
  // Enter would only insert a newline, and the form would never submit.
  input.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    // Mid-composition Enter is the IME accepting a candidate, not the reader
    // sending — swallowing it there would lose the word being written.
    if (event.isComposing) return;

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
      case "reply-hadith":
        stageReply(id);
        break;
      case "clear-reply":
        clearReply();
        break;
      case "close-drawer":
        closeOverlays();
        break;
    }
  });

  backdrop.addEventListener("click", closeOverlays);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") closeOverlays();
  });
})();
