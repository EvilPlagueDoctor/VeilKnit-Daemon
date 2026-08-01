const search = document.querySelector('#search');
if (search) {
  search.addEventListener('input', () => {
    const q = search.value.trim().toLowerCase();
    document.querySelectorAll('[data-search]').forEach(el => {
      el.classList.toggle('hidden', q && !el.dataset.search.toLowerCase().includes(q));
    });
  });
}
