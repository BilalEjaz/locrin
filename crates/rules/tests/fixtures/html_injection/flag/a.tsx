export function Article(props: { html: string }) {
  return <div dangerouslySetInnerHTML={{ __html: props.html }} />;
}

export function render(el: HTMLElement, body: string): void {
  el.innerHTML = body;
}

export function replace(el: HTMLElement, name: string): void {
  el.outerHTML = `<b>${name}</b>`;
}

export function append(el: HTMLElement, row: string): void {
  el.insertAdjacentHTML("beforeend", "<li>" + row + "</li>");
}

export function banner(message: string): void {
  document.write(message);
}

export function fill(selector: string, markup: string): void {
  $(selector).html(markup);
}

export function decode(el: HTMLElement, raw: string): void {
  el.innerHTML = _.unescape(raw);
}
