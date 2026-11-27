// Sends a card's button without a page load. Each server reply is a new
// snapshot of the board, and the page takes its layout from the snapshot.
// A card that waits on its own request, or shows an error, keeps its element.
"use strict";

let pending = 0;
let stale = false;

function kept(card) {
    return card.classList.contains("locked") || card.querySelector(".error");
}

function follow(text) {
    const main = document.querySelector("main");
    const fresh = new DOMParser().parseFromString(text, "text/html").querySelector("main");
    if (!main || !fresh) {
        return;
    }
    for (const card of [...fresh.querySelectorAll("article")]) {
        const old = document.getElementById(card.id);
        if (old && kept(old)) {
            old.setAttribute("style", card.getAttribute("style") || "");
            card.replaceWith(old);
        }
    }
    for (const old of [...main.querySelectorAll("article")]) {
        if (kept(old)) {
            fresh.append(old);
        }
    }
    main.replaceWith(document.adoptNode(fresh));
}

// A reply can predate another request's change, so once no request waits,
// one more snapshot sets every card right.
async function settle() {
    pending -= 1;
    if (pending > 0 || !stale) {
        return;
    }
    stale = false;
    const reply = await fetch("/");
    if (reply.ok && pending === 0) {
        follow(await reply.text());
    }
}

async function send(event) {
    const form = event.target;
    const card = form.closest("article");
    if (!card || card.classList.contains("locked")) {
        return;
    }
    event.preventDefault();
    const button = event.submitter;
    const body = new URLSearchParams(new FormData(form, button));
    const to = (button && button.getAttribute("formaction")) || form.getAttribute("action");
    card.classList.add("locked");
    card.querySelector(".error")?.remove();
    pending += 1;
    try {
        const reply = await fetch(to, { method: "POST", body });
        const text = await reply.text();
        if (!reply.ok) {
            throw new Error(text);
        }
        card.classList.remove("locked");
        stale = true;
        follow(text);
    } catch (error) {
        const note = document.createElement("p");
        note.className = "error";
        note.textContent = error.message;
        card.append(note);
        card.classList.remove("locked");
    }
    await settle();
}

document.addEventListener("submit", send);
