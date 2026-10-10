const { add } = require("./sum");

test("adds numbers", () => {
  expect(add(1, 2)).toBe(3);
});
