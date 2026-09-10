export function Static() {
  return <div dangerouslySetInnerHTML={{ __html: `<p>hello</p>` }} />;
}

export function render(el: HTMLElement, body: string): void {
  el.innerHTML = body; // locrin:allow
}
