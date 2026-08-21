import { expect, test } from '@rstest/core';
import { render, screen } from '@testing-library/react';
import App from '../src/App';

test('renders the main page', () => {
  render(<App />);
  expect(screen.getByRole('heading', { name: 'miao · Rsbuild' })).toBeInTheDocument();
  expect(screen.getByText('新的前端迁移入口已经就绪。')).toBeInTheDocument();
});
