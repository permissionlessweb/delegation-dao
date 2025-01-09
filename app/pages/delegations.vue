<script lang="ts" setup>
const { data } = useFetch(`/api/delegations`)

const expand = ref({
  openedRows: [],
  row: {}
})

const columns = [
  // { key: 'address', label: 'Address' },
  { key: 'name', label: 'Name' },
  { key: 'status', label: 'Status' },
  { key: 'total_amount', label: 'Current' },
  { key: 'new_delegations', label: 'New' },
  { key: 'diff', label: 'Diff' },
  { key: 'total_rewards', label: 'To Claim' }
]

const subcolumns = [
  { key: 'address', label: 'Address' },
  { key: 'amount', label: 'Amount' },
  { key: 'rewards', label: 'To Claim' }
]

const formattedData = computed(() => {
  return data.value.delegations.map((delegation: any) => {
    return {
      ...delegation,
      status: delegation.status.replace('BOND_STATUS_', ''),
      total_amount: delegation.total_amount.toLocaleString(),
      total_rewards: delegation.total_rewards.toLocaleString(),
      new_delegations: delegation.new_delegations.toLocaleString(),
      diff: (delegation.new_delegations - delegation.total_amount).toLocaleString()
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
            <h1 class="text-2xl font-semibold">Total Delegations</h1>
            <p class="text-lg">{{ data.total_amount.toLocaleString() }} BTSG</p>
          </UCard>

          <UCard>
            <h1 class="text-2xl font-semibold">Total Rewards</h1>
            <p class="text-lg">{{ data.total_rewards.toLocaleString() }} BTSG</p>
          </UCard>

          <UCard>
            <h1 class="text-2xl font-semibold">New Delegations</h1>
            <p class="text-lg">{{ data.new_delegations.toLocaleString() }} BTSG</p>
          </UCard>
        </div>

        <UTable
          v-model:expand="expand"
          :rows="formattedData"
          :columns="columns"
        >
          <template #expand="{ row }">
            <UTable :rows="row.delegators" :columns="subcolumns" />
          </template>
        </UTable>
      </UPageBody>
    </UPage>
  </UContainer>
</template>
