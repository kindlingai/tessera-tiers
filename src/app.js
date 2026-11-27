// Sends a card's button without a page load. The card stays locked and dim
// until GitHub answers, then moves to its place on the new board.
"use strict";

function place(card, fresh) {
    const column = fresh.closest("section");
    const home = column && document.getElementById(column.id);
    if (!home) {
        card.remove();
        return;
    }
    // The first later card in the new column that this page also shows.
    let next = fresh.nextElementSibling;
    while (next && !document.getElementById(next.id)) {
        next = next.nextElementSibling;
    }
    const before = next ? document.getElementById(next.id) : null;
    card.remove();
    home.insertBefore(document.adoptNode(fresh), before);
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
    try {
        const reply = await fetch(to, { method: "POST", body });
        const text = await reply.text();
        if (!reply.ok) {
            throw new Error(text);
        }
        const page = new DOMParser().parseFromString(text, "text/html");
        const fresh = page.getElementById(card.id);
        if (fresh) {
            place(card, fresh);
        } else {
            card.remove();
        }
    } catch (error) {
        const note = document.createElement("p");
        note.className = "error";
        note.textContent = error.message;
        card.append(note);
        card.classList.remove("locked");
    }
}

document.addEventListener("submit", send);
