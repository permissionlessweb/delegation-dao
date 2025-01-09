<script lang="ts" setup>
const { data } = useFetch(`/api/delegations`)

const expand = ref({
  openedRows: [],
  row: {}
})

const columns = [
  // { key: 'address', label: 'Address' },
  { key: 'name', label: 'Name' },
  { key: 'total_amount', label: 'Current' },
  { key: 'new_delegations', label: 'New' },
  { key: 'diff', label: 'Diff' }
]

const subcolumns = [
  { key: 'address', label: 'Address' },
  { key: 'amount', label: 'Amount' }
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
                v-if="row.status !== 'BONDED'"
                class="ml-2"
                :label="row.status"
                :color="row.status !== 'BONDED' ? 'red' : 'red'"
              />
            </a>
          </template>
          <template #diff-data="{ row }">
            <span
              class="font-bold"
              :class="{
                'text-green-500': parseFloat(row.diff) > 0,
                'text-red-500': parseFloat(row.diff) < 0
              }"
            >
              {{ row.diff }}
            </span>
          </template>
        </UTable>
      </UPageBody>
    </UPage>
  </UContainer>
</template>
