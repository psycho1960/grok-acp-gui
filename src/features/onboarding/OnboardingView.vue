<template>
  <section class="surface-card" aria-labelledby="onboarding-title">
    <div class="section-heading">
      <div>
        <p class="eyebrow">UI-ONBOARD-001</p>
        <h2 id="onboarding-title">启动检查</h2>
      </div>
      <span class="status-pill" role="status">
        <span class="status-dot" aria-hidden="true"></span>
        工程壳就绪
      </span>
    </div>

    <p class="intro">
      这是 Grok ACP GUI 的可启动基线。真实的环境探测与状态流转由后续任务接入。
    </p>

    <div class="empty-state" role="status">
      <span class="empty-icon" aria-hidden="true">···</span>
      <div>
        <strong>启动检查尚未接入</strong>
        <p>Git、Grok、版本、认证、数据库和可写目录检查将在后续任务中实现。</p>
      </div>
    </div>
  </section>
</template>

<style scoped>
.section-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}

h2,
p {
  margin-top: 0;
}

h2 {
  margin-bottom: 0;
  color: var(--ctp-text);
  font-size: 24px;
}

.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 6px 10px;
  color: var(--ctp-green);
  border: 1px solid var(--ctp-green);
  border-radius: 999px;
  font-size: 12px;
}

.status-dot {
  width: 7px;
  height: 7px;
  background: var(--ctp-green);
  border-radius: 50%;
}

.intro {
  max-width: 620px;
  margin: 18px 0 30px;
  color: var(--ctp-subtext0);
  line-height: 1.7;
}

.empty-state {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 22px;
  padding: 16px;
  border: 1px dashed var(--ctp-surface1);
  border-radius: 9px;
}

.empty-icon {
  display: grid;
  flex: 0 0 auto;
  width: 34px;
  height: 34px;
  place-items: center;
  color: var(--ctp-blue);
  background: var(--ctp-surface0);
  border-radius: 50%;
  font-weight: 800;
}

.empty-state p {
  margin: 5px 0 0;
  color: var(--ctp-subtext0);
  line-height: 1.55;
}

@media (max-width: 720px) {
  .section-heading {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
