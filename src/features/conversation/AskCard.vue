<script setup lang="ts">
import { computed, ref } from "vue";
import Button from "../../shared/ui/Button.vue";
import { agentQuestion } from "./tool-normalize";
import type { ToolCallView } from "./types";

const props = defineProps<{ tool: ToolCallView }>();
const emit = defineEmits<{
  answer: [answer: string, complete: (success: boolean) => void];
}>();
const customAnswer = ref("");
const submitted = ref("");
const submitting = ref(false);

const question = computed(() => agentQuestion(props.tool));
const choices = computed(() =>
  question.value?.choices.length
    ? question.value.choices
    : [
        { label: "是", value: "是" },
        { label: "否", value: "否" },
      ],
);

function submit(answer: string): void {
  const value = answer.trim();
  if (!value || submitted.value || submitting.value) return;
  submitting.value = true;
  emit("answer", value, (success) => {
    submitting.value = false;
    if (success) submitted.value = value;
  });
}
</script>

<template>
  <section class="ask-card" data-testid="ask-card" aria-labelledby="ask-title">
    <header>
      <span class="eyebrow">需要你的确认</span>
      <h3 id="ask-title">{{ question?.prompt }}</h3>
    </header>
    <p v-if="submitted" class="submitted" role="status">
      已提交：{{ submitted }}
    </p>
    <template v-else>
      <div class="choice-tabs" role="group" aria-label="可选回复">
        <Button
          v-for="choice in choices"
          :key="choice.value"
          variant="secondary"
          data-testid="ask-choice"
          :disabled="submitting"
          @click="submit(choice.value)"
        >
          {{ choice.label }}
        </Button>
      </div>
      <form class="custom-reply" @submit.prevent="submit(customAnswer)">
        <label for="ask-custom">其他回复</label>
        <input
          id="ask-custom"
          v-model="customAnswer"
          data-testid="ask-custom-input"
          placeholder="输入自定义回答"
        />
        <Button
          type="submit"
          :state="submitting ? 'loading' : 'default'"
          :disabled="submitting || !customAnswer.trim()"
        >
          提交
        </Button>
      </form>
    </template>
  </section>
</template>

<style scoped>
.ask-card {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-4);
  border: 1px solid var(--ctp-iris);
  border-radius: var(--radius-card);
  background: var(--ctp-surface0);
}
header { display: grid; gap: var(--space-1); }
.eyebrow { color: var(--ctp-iris); font-size: var(--font-small); font-weight: 600; }
h3 { margin: 0; color: var(--ctp-text); font-size: var(--font-body); line-height: var(--leading-normal); }
.choice-tabs { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.custom-reply { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: var(--space-2); align-items: center; }
.custom-reply label { color: var(--ctp-subtext0); font-size: var(--font-small); }
.custom-reply input { min-width: 0; min-height: var(--control-min-size); padding: 0 var(--space-3); color: var(--ctp-text); background: var(--ctp-base); border: 1px solid var(--ctp-surface1); border-radius: var(--radius-control); }
.submitted { margin: 0; color: var(--ctp-green); }
@media (max-width: 720px) { .custom-reply { grid-template-columns: 1fr; } }
</style>
