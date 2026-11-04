import Document, { Html, Head, Main, NextScript } from 'next/document';

class MyDocument extends Document {
  render() {
    return (
      <Html>
        <Head>
          {/* Runs before the app bundle so same-origin mode is set on first paint. */}
          {/* eslint-disable-next-line @next/next/no-sync-scripts */}
          <script src="/config.js" />
        </Head>

        <body>
          <Main />
          <NextScript />
        </body>
      </Html>
    );
  }
}

export default MyDocument;
