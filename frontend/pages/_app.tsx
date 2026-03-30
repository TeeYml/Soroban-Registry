import type { AppProps } from 'next/app';
import { ThemeProvider } from '@/providers/ThemeProvider';
import ToastProvider from '@/providers/ToastProvider';
import RealtimeProvider from '@/providers/RealtimeProvider';

export default function App({ Component, pageProps }: AppProps) {
  return (
    <ThemeProvider>
      <ToastProvider>
        <RealtimeProvider>
          <Component {...pageProps} />
        </RealtimeProvider>
      </ToastProvider>
    </ThemeProvider>
  );
}
