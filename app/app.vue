<script setup lang="ts">
import type { ParsedContent } from '@nuxt/content'

const { seo } = useAppConfig()

const { data: navigation } = await useAsyncData('navigation', () => fetchContentNavigation())
const { data: files } = useLazyFetch<ParsedContent[]>('/api/search.json', {
  default: () => [],
  server: false
})

useHead({
  meta: [
    { name: 'viewport', content: 'width=device-width, initial-scale=1' }
  ],
  link: [
    { rel: 'icon', type: 'image/png', href: '/favicon/favicon-16x16.png' },
    { rel: 'icon', type: 'image/png', href: '/favicon/favicon-32x32.png' },
    { rel: 'icon', type: 'image/png', href: '/favicon/favicon-96x96.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-57x57.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-60x60.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-72x72.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-76x76.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-114x114.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-120x120.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-144x144.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-152x152.png' },
    { rel: 'apple-touch-icon', type: 'image/png', href: '/favicon/apple-icon-180x180.png' }
  ],
  htmlAttrs: {
    lang: 'en'
  }
})

useSeoMeta({
  titleTemplate: `%s - ${seo?.siteName}`,
  ogSiteName: seo?.siteName,
  ogImage: 'https://delegation-dao.bitsong.io/social-card.png',
  twitterImage: 'https://delegation-dao.bitsong.io/social-card.png',
  twitterCard: 'summary_large_image'
})

provide('navigation', navigation)
</script>

<template>
  <div>
    <NuxtLoadingIndicator />

    <AppHeader />

    <UMain class="min-h-[calc(90vh-var(--header-height))]">
      <NuxtLayout>
        <NuxtPage />
      </NuxtLayout>
    </UMain>

    <AppFooter />

    <ClientOnly>
      <LazyUContentSearch :files="files" :navigation="navigation" />
    </ClientOnly>

    <UNotifications />
  </div>
</template>
