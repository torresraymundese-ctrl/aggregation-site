<template>
  <div class="toast-host" aria-live="polite">
    <TransitionGroup name="toast">
      <button
        v-for="notice in notices"
        :key="notice.id"
        :class="['toast-item', `toast-${notice.type}`]"
        type="button"
        @click="dismissNotice(notice.id)"
      >
        {{ notice.message }}
      </button>
    </TransitionGroup>
  </div>
</template>

<script setup>
import { dismissNotice, notices } from '../notify.js'
</script>
