module.exports = {
  testEnvironment: 'node',
  testMatch: ['<rootDir>/src/**/*.test.ts'],
  transform: {
    // src/api/client.ts reads Vite's import.meta.env, which CommonJS jest
    // cannot parse; its transform strips that one Vite-ism (see the file).
    '^.+\\/api\\/client\\.ts$': ['<rootDir>/jest.transform.noImportMeta.cjs', {}],
    '^.+\\.tsx?$': ['ts-jest', {
      tsconfig: { module: 'CommonJS', isolatedModules: true, composite: false },
    }],
  },
  moduleNameMapper: { '^(\\.{1,2}/.*)\\.js$': '$1' },
  collectCoverageFrom: [
    'src/store/gahStore.ts',
    'src/lib/{format,starMapLayout,skillFrontMatter,icons,workKey,planningTarget,reviewLabels}.ts',
  ],
  coverageReporters: ['text-summary'],
};
