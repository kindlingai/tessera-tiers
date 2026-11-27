// Sends a card's button without a page load. Each server reply is a new
// snapshot of the board, and the page follows the snapshot. A card that
// waits on its own request, or shows an error, keeps its element.
"use strict";

let pending = 0;
let stale = false;

function kept(card) {
    return card.classList.contains("locked") || card.querySelector(".error");
}

// Puts the cards of a snapshot in its order. With `only`, moves that card
// alone and leaves the others where they are.
function follow(page, only) {
    const seen = new Set();
    for (const column of page.querySelectorAll("section")) {
        const home = document.getElementById(column.id);
        if (!home) {
            continue;
        }
        let anchor = home.querySelector("h2");
        for (const fresh of [...column.querySelectorAll("article")]) {
            seen.add(fresh.id);
            const old = document.getElementById(fresh.id);
            if (only && fresh.id !== only) {
                if (old && old.parentNode === home) {
                    anchor = old;
                }
                continue;
            }
            let node = old;
            if (!old || !kept(old)) {
                node = document.adoptNode(fresh);
                old?.remove();
            }
            anchor.after(node);
            anchor = node;
        }
    }
    for (const card of document.querySelectorAll("main article")) {
        const mine = !only || card.id === only;
        if (mine && !seen.has(card.id) && !kept(card)) {
            card.remove();
        }
    }
}

function snapshot(text) {
    return new DOMParser().parseFromString(text, "text/html");
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
        follow(snapshot(await reply.text()), null);
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
        follow(snapshot(text), card.id);
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
