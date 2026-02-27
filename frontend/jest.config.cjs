/** Jest configuration for frontend tests */
module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
  testMatch: ['**/__tests__/**/*.test.{ts,tsx}', '**/?(*.)+(spec|test).{ts,tsx}'],
  setupFilesAfterEnv: ['<rootDir>/setupTests.ts'],
  collectCoverageFrom: [
    'lib/**/*.ts',
    'hooks/**/*.ts',
    'components/**/*.tsx',
    'services/**/*.ts',
    'utils/**/*.ts',
    '!lib/**/mock-data.ts',
    '!**/*.d.ts',
  ],
  coverageDirectory: '<rootDir>/coverage',
};
