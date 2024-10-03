export default defineAppConfig({
  ui: {
    primary: 'red',
    gray: 'zinc',
    footer: {
      bottom: {
        left: 'text-sm text-gray-500 dark:text-gray-400',
        wrapper: 'border-t border-gray-200 dark:border-gray-800'
      }
    }
  },
  seo: {
    siteName: 'BitSong Delegation DAO'
  },
  header: {
    logo: {
      alt: '',
      light: '',
      dark: ''
    },
    search: true,
    colorMode: true,
    links: [{
      'icon': 'i-simple-icons-github',
      'to': 'https://github.com/bitsongofficial',
      'target': '_blank',
      'aria-label': 'BitSong on GitHub'
    }]
  },
  footer: {
    credits: 'Copyright © 2024',
    colorMode: false,
    links: [{
      'icon': 'i-heroicons-globe-alt',
      'to': 'https://bitsong.io',
      'target': '_blank',
      'aria-label': 'BitSong Website'
    }, {
      'icon': 'i-simple-icons-discord',
      'to': 'https://discord.bitsong.io',
      'target': '_blank',
      'aria-label': 'BitSong on Discord'
    }, {
      'icon': 'i-simple-icons-x',
      'to': 'https://x.com/bitsongofficial',
      'target': '_blank',
      'aria-label': 'BitSong on X'
    }, {
      'icon': 'i-simple-icons-github',
      'to': 'https://github.com/bitsongofficial',
      'target': '_blank',
      'aria-label': 'BitSong on GitHub'
    }]
  },
  toc: {
    title: 'Table of Contents',
    bottom: {
      title: 'Community',
      edit: 'https://github.com/bitsongofficial/delegation-dao/edit/main/content',
      links: [{
        icon: 'i-heroicons-star',
        label: 'Star on GitHub',
        to: 'https://github.com/bitsongofficial/delegation-dao',
        target: '_blank'
      }]
    }
  }
})
