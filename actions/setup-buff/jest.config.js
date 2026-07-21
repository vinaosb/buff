/** @type {import('jest').Config} */
module.exports = {
  preset: "ts-jest",
  testEnvironment: "node",
  roots: ["<rootDir>/__tests__"],
  collectCoverageFrom: ["src/helpers/**/*.ts"],
  coverageThreshold: {
    global: {
      lines: 80,
    },
  },
};
