export default {
  sourceLocale: 'en',
  locales: ['en', 'ko'],
  catalogs: [
    {
      path: 'src/locales/{locale}/messages',
      include: ['src'],
    },
  ],
  format: 'po',
}
