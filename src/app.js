// Sends a card's button without a page load. Each server reply is a new
// snapshot of the board, and the page takes its layout from the snapshot.
// A card that waits on its own request, or shows an error, keeps its element.
"use strict";

let pending = 0;
let stale = false;

function kept(card) {
    return card.classList.contains("locked") || card.querySelector(".error");
}

const SVG = "http://www.w3.org/2000/svg";

// The point on box `a` that faces box `b`, relative to `origin`.
function edge(a, b, origin) {
    const y = (a.top + a.bottom) / 2 - origin.top;
    if (a.right <= b.left) {
        return [a.right - origin.left, y];
    }
    if (b.right <= a.left) {
        return [a.left - origin.left, y];
    }
    const x = (a.left + a.right) / 2 - origin.left;
    return [x, (b.top > a.top ? a.bottom : a.top) - origin.top];
}

// One line behind the cards for each pair of PRs that touch the same files.
function draw() {
    const main = document.querySelector("main");
    if (!main) {
        return;
    }
    main.querySelector("svg.links")?.remove();
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("class", "links");
    svg.setAttribute("width", main.scrollWidth);
    svg.setAttribute("height", main.scrollHeight);
    const origin = main.getBoundingClientRect();
    const done = new Set();
    for (const card of main.querySelectorAll("article")) {
        const mine = Number(card.id.slice(3));
        for (const kind of ["shares", "conflicts"]) {
            for (const other of (card.dataset[kind] || "").split(" ").filter(Boolean)) {
                const peer = document.getElementById("pr-" + other);
                const key = [Math.min(mine, other), Math.max(mine, other), kind].join(" ");
                if (!peer || done.has(key)) {
                    continue;
                }
                done.add(key);
                const a = card.getBoundingClientRect();
                const b = peer.getBoundingClientRect();
                const [x1, y1] = edge(a, b, origin);
                const [x2, y2] = edge(b, a, origin);
                const line = document.createElementNS(SVG, "line");
                line.setAttribute("class", kind);
                for (const [name, value] of Object.entries({ x1, y1, x2, y2 })) {
                    line.setAttribute(name, value);
                }
                svg.append(line);
            }
        }
    }
    main.prepend(svg);
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
    draw();
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

// Asks before a landing, and once more when CI is not green.
function confirmLanding(card, body) {
    const number = card.id.slice(3);
    const ask = `Squash #${number} into main now, past the merge queue and its checks?`;
    if (!confirm(ask)) {
        return false;
    }
    body.set("sure", "yes");
    const ci = card.dataset.ci;
    if (ci !== "success") {
        if (!confirm(`CI of #${number} is ${ci}. Land it anyway?`)) {
            return false;
        }
        body.set("red", "yes");
    }
    return true;
}

async function send(event) {
    const form = event.target;
    const hold = event.submitter?.value === "hold";
    if (hold && !confirm("Remove every PR from the merge queue and turn off auto-merge?")) {
        event.preventDefault();
        return;
    }
    const card = form.closest("article");
    if (!card || card.classList.contains("locked")) {
        return;
    }
    event.preventDefault();
    const button = event.submitter;
    const body = new URLSearchParams(new FormData(form, button));
    const to = (button && button.getAttribute("formaction")) || form.getAttribute("action");
    if (to === "/land" && !confirmLanding(card, body)) {
        return;
    }
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
        draw();
    }
    await settle();
}

document.addEventListener("submit", send);
addEventListener("resize", draw);
draw();
