const xs = [1, 2, 3];
const ys = [4, 5, 6];

const chained = xs.map((x) => { const a1 = x + 1; const a2 = a1 * 2; const a3 = a2 - 3; const a4 = a3 + 4; return ys.filter((y) => {
  const b1 = y + 1;
  const b2 = b1 * 2;
  const b3 = b2 - 3;
  const b4 = b3 + 4;
  return b4 > a4;
}); });
