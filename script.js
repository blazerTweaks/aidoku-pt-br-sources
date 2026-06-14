function updateCount(n) {
  document.querySelector(".source-count").textContent = n;
}

function filterSources() {
  const query = document.getElementById("search").value.toLowerCase();
  const items = document.querySelectorAll("#source-list ul li");
  let visible = 0;

  items.forEach((item) => {
    const name = item.querySelector(".source-name").textContent.toLowerCase();
    const url = item.querySelector(".source-url").textContent.toLowerCase();
    const match = name.includes(query) || url.includes(query);
    item.style.display = match ? "" : "none";
    if (match) visible++;
  });

  updateCount(visible);

  const empty = document.getElementById("empty");
  empty.style.display = visible === 0 ? "flex" : "none";
}

// inicializa o contador
const total = document.querySelectorAll("#source-list ul li").length;
updateCount(total);

lucide.createIcons();
