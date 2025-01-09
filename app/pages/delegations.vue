<script lang="ts" setup>
const { data } = useFetch(`/api/delegations`)

const expand = ref({
  openedRows: [],
  row: {}
})

const columns = [
  { key: 'name', label: 'Name' },
  { key: 'total_amount', label: 'Current', sortable: true },
  { key: 'new_delegations', label: 'New', sortable: true },
  { key: 'diff', label: 'Diff', sortable: true }
]

const subcolumns = [
  { key: 'address', label: 'Address' },
  { key: 'amount', label: 'Amount' }
]

const formattedData = computed(() => {
  return data.value.delegations.map((delegation) => {
    return {
      ...delegation,
      diff: delegation.new_delegations - delegation.total_amount
    }
  })
})
</script>

<template>
  <UContainer>
    <UPage>
      <UPageBody>
        <div class="flex space-x-10">
          <UCard>
            <h1 class="text-2xl font-semibold">
              Total Delegations
            </h1>
            <p class="text-lg">
              {{ data.total_amount.toLocaleString() }} BTSG
            </p>
          </UCard>

          <UCard>
            <h1 class="text-2xl font-semibold">
              Total Rewards
            </h1>
            <p class="text-lg">
              {{ data.total_rewards.toLocaleString() }} BTSG
            </p>
          </UCard>

          <UCard>
            <h1 class="text-2xl font-semibold">
              New Delegations
            </h1>
            <p class="text-lg">
              {{ data.new_delegations.toLocaleString() }} BTSG
            </p>
          </UCard>
        </div>

        <UTable
          v-model:expand="expand"
          :rows="formattedData"
          :columns="columns"
        >
          <template #expand="{ row }">
            <UCard>
              <UTable
                :rows="row.delegators"
                :columns="subcolumns"
              />
            </UCard>
          </template>
          <template #name-data="{ row }">
            <a
              target="_blank"
              class="text-black dark:text-white font-bold"
              :href="`https://mintscan.io/bitsong/validators/${row.address}`"
            >
              {{ row.name }}
              <UBadge
                v-if="row.status !== 'BOND_STATUS_BONDED'"
                class="ml-2"
                :label="row.status.replace('BOND_STATUS_', '')"
                :color="row.status !== 'BOND_STATUS_BONDED' ? 'red' : 'red'"
              />
            </a>
          </template>
          <template #total_amount-data="{ row }">
            {{ row.total_amount.toLocaleString() }}
          </template>
          <template #new_delegations-data="{ row }">
            {{ row.new_delegations.toLocaleString() }}
          </template>
          <template #diff-data="{ row }">
            <span
              class="font-bold"
              :class="{
                'text-green-500': row.diff > 0,
                'text-red-500': row.diff < 0
              }"
            >
              {{ row.diff.toLocaleString() }}
            </span>
          </template>
        </UTable>
      </UPageBody>
    </UPage>
  </UContainer>
</template>
